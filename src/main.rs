use iced::Font;
use lst::app::App;

fn main() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(Font::MONOSPACE)
        .window_size(iced::Size::new(980.0, 680.0))
        .exit_on_close_request(false)
        .run()
}
