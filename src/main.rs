mod app;
mod browsers;
#[path = "ui/delegate.rs"]
mod delegate;
mod requests;
mod ui;

fn main() {
    delegate::run();
}
