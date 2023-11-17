mod project;
#[macro_use] extern crate prettytable;

use sqlx::sqlite::SqlitePool;

extern crate clap;

use clap::{Parser, Subcommand};
use inquire::{InquireError, Select};
use project::TaskStatus;

#[derive(Parser)]
#[command(
author,
version,
about = "Project management done right in the terminal 👔 \n Database file is read from the ENV-variable TASKS_DB_FILE or defaults to tasks.db",
long_about = "Project management tool to support The Method from IDesign"
)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the database from from MS Project
    Init {
        /// The path to the MS Project file 📂
        #[arg(short, long, default_value = "tasks.csv")]
        project_file: Option<String>,
    },

    /// List tasks ✅
    Tasks {},

    /// Log work 📝
    Log {},

    /// Generate earned value chart 📈
    EarnedValue {
        /// The path to the generated chart
        #[arg(short, long, default_value = "earned_value.png")]
        outfile: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let database_file = std::env::var("PROJECT_MANAGER_DB_FILE").unwrap_or("tasks.db".to_string());
    let pool = SqlitePool::connect(&*format!("sqlite:{}", database_file)).await?;
    match &cli.command {
        Commands::Init { project_file } => {
            // Migrate database
            sqlx::migrate!().run(&pool).await?;
            println!("Database migrated");
            // get command
            project::init(
                &pool,
                project_file.clone().expect("No project file given"),
                database_file,
            ).await;
        }
        Commands::Tasks { .. } => {
            let options: Vec<&str> = vec!["Doing/Pending", "All", "Completed"];
            let ans: Result<&str, InquireError> = Select::new("What tasks?", options).prompt();

            let choice = match ans {
                Ok(choice) => match choice
                 {
                    "Doing/Pending" => TaskStatus::Pending,
                    "All" => TaskStatus::All,
                    "Completed" => TaskStatus::Completed,
                    _ => TaskStatus::All,
                },
                Err(_) => TaskStatus::All,
            };
            project::list(&pool, choice).await.expect("Could not list tasks");
        }
        Commands::Log { .. } => {
            project::log(&pool);
        }
        Commands::EarnedValue { .. } => {
            project::track(&pool);
        }
    }
    Ok(())
}
