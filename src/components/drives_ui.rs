use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::Block,
    Frame,
};

use crate::app::App;

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(ref drives) = app.drives {
        let drive_spans: Vec<Span> = drives
            .iter()
            .map(|drive| Span::raw(format!(" {}", &drive.label)).style(Style::new().cyan()))
            .collect();

        // let drive_list = ratatui::widgets::List::new(drive_spans)
        //     .block(
        //         Block::default()
        //             .title("  Drives  ")
        //             .borders(ratatui::widgets::Borders::ALL),
        //     )
        //     .style(Style::default().fg(Color::White).bg(Color::Black))
        //     .highlight_style(Style::default().add_modifier(Modifier::BOLD));

        let text = Line::from(drive_spans);
        let dlist = ratatui::widgets::Paragraph::new(text).block(
            Block::default()
                .title(" Drives ")
                .borders(ratatui::widgets::Borders::ALL),
        );

        f.render_widget(dlist, area);
    } else {
        f.render_widget(Span::raw("no drives"), area)
    }
}
