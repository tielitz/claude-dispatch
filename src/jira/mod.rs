pub mod adf;
pub mod client;

use std::path::Path;
use std::str::FromStr;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info};

use crate::config::Config;
use crate::jira::adf::adf_to_markdown;
use crate::jira::client::JiraClient;
use crate::markdown::generate_ticket_markdown;
use crate::planner;
use crate::state::StateDb;
use crate::validate_ticket_key;

/// Compute how long to sleep until the next cron tick.
fn duration_until_next(schedule: &cron::Schedule) -> Duration {
    let now = chrono::Utc::now();
    match schedule.upcoming(chrono::Utc).next() {
        Some(next) => {
            let delta = next - now;
            delta.to_std().unwrap_or(Duration::from_secs(1))
        }
        None => Duration::from_secs(60),
    }
}

pub async fn run_sync_loop(config: Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = JiraClient::new(
        &config.jira_base_url(),
        &config.jira.email,
        &config.jira.api_token,
    );

    let output_dir = config.output_dir();
    std::fs::create_dir_all(&output_dir)?;

    let schedule = cron::Schedule::from_str(&config.jira.cron_schedule).map_err(|e| {
        format!(
            "invalid cron_schedule '{}': {}",
            config.jira.cron_schedule, e
        )
    })?;

    info!(
        jql = %config.jira.jql,
        cron_schedule = %config.jira.cron_schedule,
        "Jira sync loop starting"
    );

    loop {
        if let Err(e) = sync_once(&config, &client, &output_dir).await {
            error!(error = %e, "sync_once failed");
        }

        let wait = duration_until_next(&schedule);
        debug!(wait_secs = wait.as_secs(), "sleeping until next cron tick");
        sleep(wait).await;
    }
}

async fn sync_once(
    config: &Config,
    client: &JiraClient,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!(jql = %config.jira.jql, "Polling Jira (JQL: {})", config.jira.jql);

    let tickets = client
        .search_tickets(&config.jira.jql, config.jira.fetch_limit)
        .await?;

    if tickets.is_empty() {
        info!("No matching tickets found");
        return Ok(());
    }

    let db = StateDb::open(&config.db_path())?;

    for ticket in &tickets {
        if !validate_ticket_key(&ticket.key) {
            error!(key = %ticket.key, "Jira ticket key failed validation, skipping");
            continue;
        }

        if db.is_known(&ticket.key)? {
            debug!(key = %ticket.key, "Skipping {} (already processed)", ticket.key);
            continue;
        }

        info!(
            key = %ticket.key,
            summary = %ticket.summary,
            "Picked up new Jira ticket: {} — {}",
            ticket.key,
            ticket.summary
        );

        db.insert_synced(&ticket.key, &ticket.summary)?;

        let description = match &ticket.description_adf {
            Some(adf) => {
                let md = adf_to_markdown(adf);
                if md.is_empty() {
                    "No description provided.".to_string()
                } else {
                    md
                }
            }
            None => "No description provided.".to_string(),
        };

        db.mark_planning(&ticket.key)?;

        let plan = match planner::draft_plan(config, ticket, &description).await {
            Ok(p) => {
                info!(key = %ticket.key, "Aggregated implementation plan for {}", ticket.key);
                p
            }
            Err(e) => {
                error!(key = %ticket.key, error = %e, "Failed to draft plan for {}", ticket.key);
                db.mark_failed(&ticket.key)?;
                continue;
            }
        };

        let markdown = generate_ticket_markdown(ticket, &plan);
        let file_path = output_dir.join(format!("{}.md", ticket.key));
        std::fs::write(&file_path, &markdown)?;

        let file_path_str = file_path.to_string_lossy().into_owned();
        info!(path = %file_path_str, "Written plan file: {}", file_path_str);

        db.mark_planned(&ticket.key, &file_path_str)?;
    }

    Ok(())
}
