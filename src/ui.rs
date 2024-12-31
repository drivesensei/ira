use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
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

    let Rect { width, height, .. } = frame.area();

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
            frame.area(),
        )
    } else {
        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Length(3),
                ratatui::layout::Constraint::Length(3),
                ratatui::layout::Constraint::Length(3),
                ratatui::layout::Constraint::Min(2),
            ])
            .split(frame.area());

        crate::components::drives_ui::render(frame, app, chunks[0]);
        crate::components::common_folders_ui::render(frame, app, chunks[1]);
        crate::components::bookmarks_ui::render(frame, app, chunks[2]);

        crate::components::tab1_files_ui::render(frame, app, chunks[3]);
    }
}
