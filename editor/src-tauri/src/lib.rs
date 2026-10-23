mod commands;
mod installed;
mod viewport_module;

use tauri::Manager;

use commands::{
    atlas, audio, behavior, build, build_tools, data_editor, editor_prefs, editor_settings, editor_window,
    help, packages, project, psd, references, scene, search, servers, shader_editor, spritesheet,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(std::sync::Mutex::new(audio::AudioPreviewState::new()))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .register_asynchronous_uri_scheme_protocol(viewport_module::SCHEME, |_ctx, request, responder| {
            // The wasm is megabytes: read it off the webview's thread.
            std::thread::spawn(move || responder.respond(viewport_module::respond(&request)));
        })
        .setup(|app| {
            installed::setup(app);
            // Installed in release too, not just debug: this logger is what
            // feeds the Build panel (see the format hook below), so without
            // it a shipped editor shows an empty panel for the whole build.
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .format(|out, message, record| {
                        // The builder narrates a build with ~40 `log::info!`
                        // calls - which backends, what was copied, what was
                        // generated. Those only ever reached the terminal the
                        // editor was launched from, so the panel showed the
                        // command line and then nothing. Forwarding here
                        // catches every one of them, including any added
                        // later; `emit` is a no-op when no build is running.
                        //
                        // `progress` itself is excluded: it logs and emits the
                        // same line by design, and forwarding it here would
                        // double every line of child output - and strip the
                        // redraw flag off the copy that arrived second.
                        let target = record.target();
                        if target.starts_with("jengine_builder")
                            && !target.starts_with("jengine_builder::progress")
                        {
                            jengine_builder::progress::emit(&message.to_string());
                        }
                        out.finish(format_args!(
                            "[{}][{}] {message}",
                            record.target(),
                            record.level()
                        ));
                    })
                    .build(),
            )?;
            // The main window is made here, not by Tauri from the config
            // (`create: false`), so its browser arguments can follow the
            // editor's GPU preference (`editor_prefs`).
            editor_prefs::create_main_window(app)?;
            // Let editor shortcuts on F3 / F5 / F12 / Ctrl+R reach the page
            // instead of being eaten by WebView2. The frontend Debug menu
            // (and its key bindings) provide reload + devtools in return.
            for window in app.webview_windows().values() {
                editor_window::disable_browser_accelerator_keys(window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            editor_prefs::get_editor_prefs,
            editor_prefs::set_editor_prefs,
            scene::create_new_scene,
            scene::load_scene,
            scene::save_scene,
            scene::add_entity,
            scene::remove_entity,
            scene::update_entity_name,
            scene::load_script_sources,
            project::create_project,
            project::open_project,
            project::save_project,
            project::refresh_project_files,
            project::list_assets,
            search::search_in_files,
            search::preview_replace_in_files,
            search::apply_replace_in_files,
            project::list_project_images,
            project::list_asset_tree,
            project::list_prefabs,
            project::create_asset_folder,
            project::delete_asset,
            project::rename_asset,
            references::find_asset_references,
            references::rename_asset_with_references,
            references::rewrite_asset_references_in_text,
            project::import_asset,
            project::duplicate_asset,
            project::create_asset_file,
            project::get_recent_projects,
            project::remove_recent_project,
            project::read_asset_binary,
            project::open_in_code_editor,
            project::show_in_file_manager,
            build::build_windows,
            build::build_web,
            build::build_desktop,
            build::build_ios,
            build::build_android,
            build::open_build_report,
            build::open_build_folder,
            build::viewport_package_status,
            build::rebuild_project_viewport,
            build::switch_project_viewport,
            build::build_and_run_native,
            build::stop_native_instances,
            build::native_instances_running,
            servers::server_config,
            servers::server_start,
            servers::server_stop,
            servers::server_status,
            servers::server_notice,
            servers::server_open_dashboard,
            servers::fleet_create,
            servers::fleet_status,
            servers::fleet_deploy,
            servers::fleet_act,
            servers::fleet_logs,
            servers::fleet_local_agent,
            servers::fleet_open_console,
            editor_window::toggle_devtools,
            help::about_info,
            help::open_external_url,
            help::logs_folder,
            help::open_logs_folder,
            build_tools::check_build_tools,
            build_tools::install_build_tool,
            editor_settings::load_editor_settings,
            editor_settings::save_editor_settings,
            psd::import_psd_layers,
            spritesheet::pack_spritesheet,
            spritesheet::save_spritesheet,
            spritesheet::compress_texture,
            spritesheet::preview_atlas,
            spritesheet::estimate_savings,
            spritesheet::list_atlas_entries,
            atlas::create_atlas,
            atlas::load_atlas,
            atlas::save_atlas,
            atlas::get_image_dimensions,
            atlas::load_atlas_image,
            atlas::list_atlas_def_images,
            atlas::list_atlas_def_animations,
            packages::list_packages,
            packages::add_package,
            packages::remove_package,
            packages::resolve_packages,
            packages::get_package_editor_schema,
            packages::get_available_packages,
            packages::read_package_panel_html,
            packages::detect_package_info,
            packages::preview_rust_package,
            packages::create_rust_package,
            shader_editor::shader_get_palette,
            shader_editor::shader_validate,
            shader_editor::shader_generate,
            shader_editor::shader_generate_preview_glsl,
            shader_editor::shader_load,
            shader_editor::shader_save,
            shader_editor::shader_create_new,
            shader_editor::shader_get_node_pins,
            audio::play_audio_preview,
            audio::stop_audio_preview,
            audio::pause_audio_preview,
            audio::resume_audio_preview,
            audio::set_audio_preview_volume,
            behavior::behavior_op_catalog,
            behavior::compile_behavior,
            behavior::write_behavior,
            data_editor::load_data_schema,
            data_editor::save_data_schema,
            data_editor::load_data_table,
            data_editor::save_data_table,
            data_editor::list_data_schemas,
            data_editor::list_data_tables_for_schema,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // A test server or fleet agent the Servers panel started does not
            // outlive the editor: the server is asked to shut down (everyone
            // saved), the agent stops what it runs.
            if let tauri::RunEvent::Exit = event {
                servers::shutdown();
                build::shutdown();
            }
        });
}
