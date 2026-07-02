#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    node_watcher_lib::run();
}
