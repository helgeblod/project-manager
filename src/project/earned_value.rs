use std::fs;
use std::path::PathBuf;

use chrono::{Datelike, Duration, Local, NaiveDate};
use inquire::error::InquireResult;
use inquire::{Confirm, Select};
use plotly::color::NamedColor::{Blue, Green, Orange, Red};
use plotly::common::{Mode, Title};
use plotly::layout::Axis;
use plotly::{common, ImageFormat, Layout, Plot, Scatter};
use promptly::prompt_default;
use sqlx::{Pool, Sqlite};

pub(crate) async fn generate_chart(pool: &Pool<Sqlite>, chart_title: &str) -> anyhow::Result<()> {
    // Insert the task, then obtain the ID of this row
    let dates = sqlx::query!(
        r#"
        SELECT min(start_date) as start_date, max(finish_date) as end_date FROM tasks
        "#,
    )
    .fetch_one(pool)
    .await?;

    let start_date = dates.start_date.expect("No start date");
    let end_date = dates.end_date.expect("No end date");

    // Generate weeks between start and end date
    let week_numbers = generate_week_numbers(
        NaiveDate::parse_from_str(&start_date, "%Y-%m-%d").expect("Error parsing start date"),
        NaiveDate::parse_from_str(&end_date, "%Y-%m-%d").expect("Error parsing end date"),
    );

    let total_effort_result = sqlx::query!(
        r#"
        SELECT sum(duration) as total_effort FROM tasks
        "#,
    )
    .fetch_one(pool)
    .await?;

    let total_effort = total_effort_result.total_effort.expect("No total effort");

    let tasks = sqlx::query!(
        r#"SELECT duration,
           CAST(CASE
               WHEN strftime('%Y%W', finish_date) < 10 THEN '0' || strftime('%W', finish_date)
               ELSE strftime('%Y%W', finish_date)
               END AS INTEGER) AS should_finish,
           CAST(CASE
               WHEN strftime('%Y%W', finished_at) < 10 THEN '0' || strftime('%W', finished_at)
               ELSE strftime('%Y%W', finished_at)
               END AS INTEGER) AS actual_finish
           FROM tasks
             LEFT JOIN task_data td on tasks.id = td.task_id
           WHERE duration > 0
           ORDER BY start_date, total_slack DESC;
    "#,
    )
    .fetch_all(pool)
    .await?;

    let work_effort = sqlx::query!(
        r#"SELECT sum(duration) as effort,
        CAST(CASE
                WHEN strftime('%Y%W', date) < 10 THEN '0' || strftime('%W', date)
                ELSE strftime('%Y%W', date)
           END AS INTEGER) AS week
        FROM timesheet
        GROUP BY week
        ORDER BY week ASC;
    "#,
    )
    .fetch_all(pool)
    .await?;

    let mut planned_value = vec![0.0f32; week_numbers.len()];
    let mut earned_value = vec![0.0f32; week_numbers.len()];
    let mut effort = vec![0.0f32; week_numbers.len()];

    for task in tasks {
        let task_value = (task.duration as f32 / total_effort as f32) * 100f32;

        let value_planned = task_value;

        let value_actual = match task.actual_finish {
            None => 0.0,
            Some(_) => task_value,
        };

        // Planned value
        if let Some(index) = week_numbers
            .iter()
            .position(|&x| x == task.should_finish.unwrap())
        {
            planned_value[index] += value_planned;
        }

        // Earned value
        if task.actual_finish.is_some() {
            let index = week_numbers
                .iter()
                .position(|&x| x == task.actual_finish.unwrap() as i32)
                .unwrap();
            earned_value[index] += value_actual;
        }
    }

    // aggregate planned_value
    for i in 1..planned_value.len() {
        planned_value[i] += planned_value[i - 1];
    }

    // aggregate planned_value
    let date = Local::now();
    let year = date.year();
    let week = date.iso_week().week();
    let current_week = (year.to_string() + format!("{:02}", week).as_str())
        .parse()
        .unwrap();

    for i in 1..earned_value.len() {
        if week_numbers[i] > current_week {
            earned_value[i] = 0.0;
        } else {
            earned_value[i] += earned_value[i - 1];
        }
    }

    // Remove trailing zeroes
    earned_value = earned_value
        .into_iter()
        .rev()
        .skip_while(|&x| x == 0.0)
        .collect();
    earned_value.reverse();

    // Effort
    for work_line in work_effort {
        let week = work_line.week.unwrap() as i32;
        let eff = work_line.effort.unwrap() as f32;
        let index = week_numbers.iter().position(|&x| x == week).unwrap();
        effort[index] += (eff / total_effort as f32) * 100.0;
    }

    // aggregate effort
    for i in 1..effort.len() {
        if week_numbers[i] > current_week {
            effort[i] = 0.0;
        } else {
            effort[i] += effort[i - 1];
        }
    }

    // Remove trailing zeroes
    effort = effort.into_iter().rev().skip_while(|&x| x == 0.0).collect();
    effort.reverse();

    let week_prefix = "W";
    let x_axis: Vec<String> = week_numbers
        .iter()
        .map(|&num| week_prefix.to_owned() + &num.to_string().get(2..).unwrap_or_default())
        .collect();

    let trace1 = Scatter::new(x_axis.clone(), effort)
        .mode(Mode::Lines)
        .line(common::Line::new().color(Red))
        .name("Effort 🧑‍💻");

    let trace2 = Scatter::new(x_axis.clone(), planned_value)
        .mode(Mode::Lines)
        .line(common::Line::new().color(Blue))
        .name("Planned Progress 🏦");
    let trace3 = Scatter::new(x_axis.clone(), earned_value)
        .mode(Mode::Lines)
        .line(common::Line::new().color(Green))
        .name("Earned value 💶");

    let layout = Layout::new()
        .title(Title::new(chart_title))
        .x_axis(Axis::new().title(Title::from("Week #")))
        .y_axis(Axis::new().title(Title::from("Done %")));

    let mut plot = Plot::new();
    plot.add_trace(trace1);
    plot.add_trace(trace2);
    plot.add_trace(trace3);
    plot.set_layout(layout);

    let options = vec![ImageFormat::PDF, ImageFormat::SVG, ImageFormat::PNG];
    let ans = Select::new("Output file format?", options).prompt();

    let image_format = if ans.is_err() {
        println!("Could not get output file format, defaulting to PDF");
        ImageFormat::PDF
    } else {
        match ans.unwrap() {
            ImageFormat::PNG => ImageFormat::PNG,
            ImageFormat::SVG => ImageFormat::SVG,
            ImageFormat::PDF => ImageFormat::PDF,
            _ => {
                println!("Unsupported file format, defaulting to PDF");
                ImageFormat::PDF
            }
        }
    };

    // Generate outfile
    let today = Local::now();
    let prefixed_file_name = format!(
        "charts/ev_chart-week-{}-({}).{image_format}",
        today.iso_week().week(),
        today.format("%s").to_string()
    );

    let path = PathBuf::from(prefixed_file_name);
    let out_file: PathBuf = prompt_default("Enter path to generated chart:", path)?;
    fs::create_dir_all(&out_file.parent().unwrap().to_path_buf())?;

    match image_format {
        ImageFormat::PNG => plot.write_image(out_file.clone(), ImageFormat::PNG, 1800, 1000, 1.0),
        ImageFormat::SVG => plot.write_image(out_file.clone(), ImageFormat::SVG, 1800, 1000, 1.0),
        ImageFormat::PDF => plot.write_image(out_file.clone(), ImageFormat::PDF, 1800, 1000, 1.0),
        _ => plot.write_image(out_file.clone(), ImageFormat::PDF, 1800, 1000, 1.0),
    };

    let ans = Confirm::new("Open the generated Earned Value Chart?")
        .with_default(false)
        .prompt();

    let file = out_file.clone();

    if ans.is_ok() && ans.unwrap() == true {
        opener::open(file).expect("Could not open file");
    } else {
        println!(
            "Ok, the chart is saved in the charts folder: {}",
            file.to_str().unwrap()
        );
    }

    // To open in browser
    // plot.show()
    Ok(())
}

fn generate_week_numbers(start_date: NaiveDate, finish_date: NaiveDate) -> Vec<i32> {
    let mut week_numbers = Vec::new();
    let mut current_date = start_date;

    while current_date <= finish_date {
        let year = current_date.year(); // Get the last two digits of the year
        let week_number = current_date.iso_week().week();
        let compound_week = format!("{}{:02}", year, week_number)
            .parse::<i32>()
            .unwrap();
        week_numbers.push(compound_week);
        current_date += Duration::weeks(1);
    }
    week_numbers
}
