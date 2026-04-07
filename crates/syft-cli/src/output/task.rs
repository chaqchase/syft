use anyhow::Result;
use syft_types::Task;

use super::emit;

pub fn emit_task(as_json: bool, task: &Task) -> Result<()> {
    emit(
        as_json,
        task,
        &format!("task {} created: {}", task.id, task.title),
    )
}

pub fn emit_task_detail(as_json: bool, task: &Task) -> Result<()> {
    if as_json {
        return emit(true, task, "");
    }

    println!("task: {} ({})", task.title, task.id);
    println!("status: {:?}  priority: {:?}", task.status, task.priority);
    println!(
        "description: {}",
        if task.description.is_empty() {
            "<none>"
        } else {
            &task.description
        }
    );
    println!(
        "acceptance: {}",
        if task.acceptance_criteria.is_empty() {
            "<none>".to_string()
        } else {
            task.acceptance_criteria.join(" | ")
        }
    );
    println!(
        "constraints: {}",
        if task.constraints.is_empty() {
            "<none>".to_string()
        } else {
            task.constraints.join(" | ")
        }
    );
    println!(
        "labels: {}",
        if task.labels.is_empty() {
            "<none>".to_string()
        } else {
            task.labels.join(", ")
        }
    );
    println!("created: {}", task.created_at.to_rfc3339());
    println!("updated: {}", task.updated_at.to_rfc3339());
    Ok(())
}

pub fn emit_tasks(as_json: bool, tasks: &[Task]) -> Result<()> {
    if as_json {
        return emit(true, &tasks, "");
    }

    if tasks.is_empty() {
        println!("no tasks");
        return Ok(());
    }

    for task in tasks {
        println!(
            "{}  {}  {:?}  {:?}",
            task.id, task.title, task.status, task.priority
        );
    }
    Ok(())
}

pub fn emit_optional_task(as_json: bool, task: Option<&Task>) -> Result<()> {
    match task {
        Some(task) => emit_task_detail(as_json, task),
        None if as_json => {
            println!("null");
            Ok(())
        }
        None => {
            println!("no current task");
            Ok(())
        }
    }
}

pub fn emit_current_task_set(as_json: bool, task: &Task) -> Result<()> {
    if as_json {
        emit(true, task, "")
    } else {
        println!("current task set: {} ({})", task.title, task.id);
        Ok(())
    }
}
