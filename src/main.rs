use adw::prelude::*;
use gtk::glib;

mod app;
pub mod telegram;

fn main() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id("io.github.sakibshahariar.Telestra")
        .build();

    application.connect_activate(app::build);
    application.run()
}
