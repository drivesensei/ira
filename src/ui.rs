use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Paragraph},
    Frame,
};

use crate::app::App;

/// Renders the user interface widgets.
pub fn render(app: &mut App, frame: &mut Frame) {
    // This is where you add new widgets.
    // See the following resources:
    // - https://docs.rs/ratatui/latest/ratatui/widgets/index.html
    // - https://github.com/ratatui-org/ratatui/tree/master/examples

    let Rect { width, height, .. } = frame.size();
    let app_title_block = Block::bordered()
        .title("     IRA (Integrated Retro Archives)    ")
        .title_alignment(Alignment::Center)
        .title_style(Style::new().add_modifier(Modifier::BOLD))
        .border_type(BorderType::Rounded);

    if app.should_increase_size(width, height) {
        frame.render_widget(
            Paragraph::new(format!("Please increase the terminal's size"))
                .block(app_title_block)
                .style(Style::default().fg(Color::Cyan).bg(Color::Black))
                .centered(),
            frame.size(),
        )
    } else {
        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .margin(1)
            .constraints(
                [
                    ratatui::layout::Constraint::Percentage(50),
                    ratatui::layout::Constraint::Percentage(50),
                ]
                .as_ref(),
            )
            .split(frame.size());

        if let Some(drives) = &app.drives {
            let drive_spans: Vec<Span> = drives
                .iter()
                .map(|drive| Span::raw(format!(" {}", &drive.label)))
                .collect();

            let drive_list = ratatui::widgets::List::new(drive_spans)
                .block(
                    Block::default()
                        .title("  Drives  ")
                        .borders(ratatui::widgets::Borders::ALL),
                )
                .style(Style::default().fg(Color::White).bg(Color::Black))
                .highlight_style(Style::default().add_modifier(Modifier::BOLD));

            frame.render_widget(drive_list, chunks[0]);
        }

        // list of files
        if let Ok(files) = app.list_files_from_selected_folder() {
            let label = &app.current_drive.as_ref().unwrap().label;

            let file_spans: Vec<Span> = files
                .iter()
                .map(|file| Span::raw(format!(" {}", &file.label)))
                .collect();

            let file_list = ratatui::widgets::List::new(file_spans)
                .block(
                    Block::default()
                        .title(format!("  Files  ({})", label))
                        .borders(ratatui::widgets::Borders::ALL),
                )
                .style(Style::default().fg(Color::White).bg(Color::Black))
                .highlight_style(Style::default().add_modifier(Modifier::BOLD));

            frame.render_widget(file_list, chunks[1]);
        }
    }
}
