use chrono::{Datelike, Duration, Local, NaiveDate};
use plotly::color::NamedColor::Green;
use plotly::common::{Mode, Title};
use plotly::layout::Axis;
use plotly::{common, ImageFormat, Layout, Plot, Scatter};
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;

pub(crate) async fn generate_chart(pool: &Pool<Sqlite>, out_file: PathBuf) -> anyhow::Result<()> {
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
    println!("Week numbers: {:?}", week_numbers);

    let total_effort_result = sqlx::query!(
        r#"
        SELECT sum(duration) as total_effort FROM tasks
        "#,
    )
    .fetch_one(pool)
    .await?;

    let total_effort = total_effort_result.total_effort.expect("No total effort");
    println!("Total effort: {}", total_effort);

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

    println!("Tasks: {:?}", tasks);

    let mut planned_value = vec![0.0f32; week_numbers.len()];
    let mut earned_value = vec![0.0f32; week_numbers.len()];
    for task in tasks {
        let task_value = (task.duration as f32 / total_effort as f32) * 100f32;

        let value_planned = task_value;

        let value_actual = match task.actual_finish {
            None => 0.0,
            Some(_) => task_value,
        };

        println!(
            "#{} Planned value: {}% actual_value: {}%",
            task.duration, value_planned, value_actual
        );

        // Planned value
        if let Some(index) = week_numbers
            .iter()
            .position(|&x| x == task.should_finish.unwrap())
        {
            planned_value[index] += value_planned;
            println!("Element found at index {}", index);
        } else {
            println!("Element not found in vec");
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

    println!("Planned value: {:?}", planned_value);
    println!("Earned value: {:?}", earned_value);

    let week_prefix = "W";
    let x_axis: Vec<String> = week_numbers
        .iter()
        .map(|&num| week_prefix.to_owned() + &num.to_string().get(2..).unwrap_or_default())
        .collect();

    let trace2 = Scatter::new(x_axis.clone(), planned_value)
        .mode(Mode::Lines)
        .name("Planned Progress");
    let trace3 = Scatter::new(x_axis.clone(), earned_value)
        .mode(Mode::Lines)
        .line(common::Line::new().color(Green))
        .name("Earned value");

    let layout = Layout::new()
        .title(Title::new("Earned Value Chart ✨"))
        .x_axis(Axis::new().title(Title::from("Week #")))
        .y_axis(Axis::new().title(Title::from("Done %")));

    let mut plot = Plot::new();
    plot.add_trace(trace2);
    plot.add_trace(trace3);
    plot.set_layout(layout);
    plot.write_image(out_file, ImageFormat::PNG, 1800, 1000, 1.0);
    // To open in browser
    // plot.show();
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

#[derive(Debug)]
struct Task {
    duration: i32,
    planned_finish: i32,
    actual_finish: Option<i32>,
}
