extern crate clap;
extern crate prettytable;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use chrono::Datelike;
use clap::{Parser, Subcommand};
use inquire::{InquireError, Select};
use promptly::prompt_default;
use sqlx::migrate::MigrateDatabase;
use sqlx::sqlite::SqlitePool;
use sqlx::{Pool, Sqlite};

use project::{earned_value, TaskStatus};

mod project;

#[derive(Parser)]
#[command(
    author,
    version,
    about = "Project management done right in the terminal 👔 \n Database file is read from the ENV-variable PROJECT_MANAGER_DB_FILE or defaults to ./db/tasks.db",
    long_about = "Project management tool to support The Method from IDesign"
)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Complete a task
    Complete {},

    /// Import project from MS Project
    Import {},

    /// List tasks ✅
    List {
        /// Filter tasks
        #[clap(short, long)]
        number_of_tasks: Option<usize>,
    },

    /// Log work
    log {},

    /// Assign task
    Assign {},

    /// Generate earned value chart
    EV {
        #[clap(short, long)]
        chart_title: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let database_file =
        std::env::var("PROJECT_MANAGER_DB_FILE").unwrap_or("./db/tasks.db".to_string());

    match &cli.command {
        Commands::Import {} => {
            // Migrate database
            create_database_check(&database_file).await?;
            let pool = connect_to_db_pool(&database_file).await;
            sqlx::migrate!().run(&pool).await?;
            // Prompt Y/n with a default value when input is empty
            let confirm_continue = prompt_default("💩 Continue importing from MS Project?", false);
            if confirm_continue.is_err() || confirm_continue.unwrap() != true {
                println!("👋 Bye!");
                return Ok(());
            }

            let project_file: PathBuf =
                prompt_default("Enter path to MS Project file", PathBuf::from("tasks.csv"))?;

            // get command
            project::import(
                &pool,
                project_file.to_string_lossy().into_owned(),
                database_file,
            )
            .await;
        }

        Commands::List { number_of_tasks } => {
            let pool = connect_to_db_pool(&database_file).await;

            let options: Vec<&str> = vec!["Assigned", "Unassigned", "All", "Completed"];
            let ans: Result<&str, InquireError> = Select::new("What tasks?", options).prompt();

            let choice = match ans {
                Ok(choice) => match choice {
                    "Assigned" => TaskStatus::Assigned,
                    "Unassigned" => TaskStatus::Unassigned,
                    "All" => TaskStatus::All,
                    "Completed" => TaskStatus::Completed,
                    _ => TaskStatus::All,
                },
                Err(_) => TaskStatus::All,
            };
            project::list(&pool, choice, number_of_tasks)
                .await
                .expect("Could not list tasks");
        }

        Commands::log { .. } => {
            let pool = connect_to_db_pool(&database_file).await;
            project::log_work(&pool).await.expect("Could not log work");
        }

        Commands::EV { chart_title } => {
            let pool = connect_to_db_pool(&database_file).await;

            let title = chart_title
                .clone()
                .unwrap_or("Earned value chart ✨".to_string());

            earned_value::generate_chart(&pool, title.as_str())
                .await
                .expect("Could not generate chart 💥");
        }

        Commands::Complete {} => {
            let pool = connect_to_db_pool(&database_file).await;
            project::complete_tasks(&pool).await?;
        }
        Commands::Assign { .. } => {
            let pool = connect_to_db_pool(&database_file).await;
            project::assign_tasks(pool).await?;
        }
    }
    Ok(())
}

async fn connect_to_db_pool(database_file: &String) -> Pool<Sqlite> {
    let pool = SqlitePool::connect(&*db_url(database_file))
        .await
        .expect(&*format!(
            "Could not connect to database {}, check if correct folder!",
            database_file
        ));
    pool
}

async fn create_database_check(database_file: &String) -> anyhow::Result<()> {
    if !Sqlite::database_exists(&*db_url(database_file)).await? {
        // Create the parent directory if it doesn't exist
        if let Some(parent_dir) = Path::new(&database_file).parent() {
            fs::create_dir_all(parent_dir)?;
        }
        Sqlite::create_database(&*db_url(database_file)).await?;
    } else {
        println!(
            "👋 Database exists, remove and re-run init (using db: {}).",
            database_file
        );
        exit(1);
    }
    Ok(())
}

fn db_url(database_file: &String) -> String {
    return format!("sqlite:{}", database_file);
}
