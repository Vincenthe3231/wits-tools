//! Registers the native commands and plugins for the desktop workspace.
mod api_tester;
mod markdown_editor;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(markdown_editor::EditorState::default())
        .invoke_handler(tauri::generate_handler![
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
