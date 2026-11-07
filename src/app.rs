use adw::prelude::*;

use crate::telegram::BackendStatus;

pub fn build(application: &adw::Application) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.append(&adw::HeaderBar::new());

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(48)
        .margin_bottom(48)
        .margin_start(32)
        .margin_end(32)
        .build();

    let title = gtk::Label::builder()
        .label("Telestra")
        .css_classes(["title-1"])
        .build();

    let summary = gtk::Label::builder()
        .label("A native Telegram client for Linux")
        .build();

    let status = gtk::Label::builder()
        .label(BackendStatus::NotConfigured.description())
        .wrap(true)
        .build();

    content.append(&title);
    content.append(&summary);
    content.append(&status);
    root.append(&content);

    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("Telestra")
        .default_width(980)
        .default_height(680)
        .content(&root)
        .build();

    window.present();
}
