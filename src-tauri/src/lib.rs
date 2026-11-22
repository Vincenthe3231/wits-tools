//! Registers the native commands and plugins for the desktop workspace.
mod api_tester;
pub mod crawler;
mod markdown_editor;
mod prototype_builder;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let handle: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        markdown_editor::commands::editor_open,
        markdown_editor::commands::editor_save,
        // Keep image operations scoped to the editor session.
        markdown_editor::commands::editor_stage_image,
        markdown_editor::commands::editor_read_image,
        markdown_editor::commands::editor_reset,
        markdown_editor::commands::editor_recovery_info,
        markdown_editor::commands::editor_restore,
        markdown_editor::commands::editor_update_recovery,
        // Keep HTTP execution behind the API Tester command.
        api_tester::api_tester_send,
        // Expose only starter import and local snapshot operations to the builder.
        prototype_builder::prototype_import,
        prototype_builder::prototype_save,
        prototype_builder::prototype_list,
        // Keep crawls, media transfers, and capture sessions behind typed native commands.
        crawler::commands::crawler_start,
        crawler::commands::crawler_jobs,
        crawler::commands::crawler_results,
        crawler::commands::crawler_stop,
        crawler::commands::crawler_delete,
        crawler::commands::crawler_export,
        crawler::commands::crawler_pick_folder,
        crawler::commands::crawler_download,
        crawler::commands::crawler_downloads,
        crawler::commands::crawler_cancel_download,
        crawler::commands::crawler_open_capture_browser,
        crawler::commands::crawler_capture_browser_sources,
        crawler::commands::crawler_shutdown,
    ];
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(markdown_editor::EditorState::default())
        .manage(crawler::commands::DesktopState::default())
        .manage(prototype_builder::GitHubClient::new())
        // External pages opened for media capture never receive application commands.
        .invoke_handler(move |invoke: tauri::ipc::Invoke<tauri::Wry>| {
            if invoke.message.webview().label() == crawler::capture_browser::LABEL {
                invoke
                    .resolver
                    .reject("Application commands are unavailable in the capture browser.");
                return true;
            }
            handle(invoke)
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
