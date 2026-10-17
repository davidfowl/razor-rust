use razor_slices::{html, partial, HtmlString, Slice};

pub struct Task {
    pub title: String,
    pub complete: bool,
    pub due: Option<String>,
}

pub struct Group {
    pub name: String,
    pub tasks: Vec<Task>,
}

pub fn grouped_tasks(groups: &[Group]) -> impl Slice + '_ {
    let footer = html! { {HtmlString::from("<footer>Reusable control flow.</footer>\n")} };
    html! {
        "<main>\n"
        for group in groups {
            "<section><h2>" {group.name} "</h2>\n<ul>\n"
            for task in &group.tasks {
                "<li>" {task.title} ": "
                if task.complete {
                    "Completed"
                } else if let Some(due) = &task.due {
                    "Due " {due}
                } else {
                    "Unscheduled"
                }
                "</li>\n"
            }
            "</ul>\n"
            if group.tasks.is_empty() {
                "<p>No tasks.</p>\n"
            }
            "</section>\n"
        }
        {partial(&footer)}
        "</main>\n"
    }
}

pub fn sample_groups() -> Vec<Group> {
    vec![
        Group {
            name: "Home <&>".into(),
            tasks: vec![
                Task {
                    title: "Wash <dishes> & \"cups\"".into(),
                    complete: true,
                    due: Some("ignored".into()),
                },
                Task {
                    title: "Call 'Grandma' 世界".into(),
                    complete: false,
                    due: Some("2026-10-14 <soon>".into()),
                },
                Task {
                    title: "Rest".into(),
                    complete: false,
                    due: None,
                },
            ],
        },
        Group {
            name: "Empty".into(),
            tasks: vec![],
        },
    ]
}
