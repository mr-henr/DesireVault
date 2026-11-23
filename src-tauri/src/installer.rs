use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::mpsc,
    thread,
    time::Duration,
};

const APP_BINARY: &[u8] = include_bytes!("../../outputs/GameLibraryManager/game-library-manager");
const APP_ICON: &[u8] = include_bytes!("../../outputs/GameLibraryManager/icon.png");
const BRAND_LOGO: &[u8] = include_bytes!("../../src/assets/desire-vault-logo-horizontal.svg");

#[derive(Clone)]
struct Progress {
    percent: f64,
    message: String,
    done: bool,
    error: Option<String>,
}

fn desktop_dir(home: &Path) -> Result<PathBuf, String> {
    let path = Command::new("xdg-user-dir")
        .arg("DESKTOP")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("Desktop"));
    if !path.is_dir() {
        return Err(format!(
            "A pasta da área de trabalho não existe: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn uninstall(remove_user_data: bool, send: &mpsc::Sender<Progress>) -> Result<(), String> {
    let home =
        PathBuf::from(env::var_os("HOME").ok_or("Não encontrei a pasta pessoal do usuário.")?);
    let app_data = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join("com.flavio.gamelibrarymanager");
    let user_data_dir = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let paths = [
        home.join(".local/bin/desire-vault"),
        user_data_dir.join("applications/desire-vault.desktop"),
        user_data_dir.join("icons/hicolor/256x256/apps/desire-vault.png"),
        desktop_dir(&home)
            .ok()
            .map(|dir| dir.join("Desire Vault.desktop"))
            .unwrap_or_default(),
    ];
    for (index, path) in paths.iter().enumerate() {
        if !path.as_os_str().is_empty() {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("Não consegui remover {}: {error}", path.display()))
                }
            }
        }
        send.send(Progress {
            percent: 25.0 + index as f64 * 15.0,
            message: "Removendo o aplicativo e seus atalhos…".into(),
            done: false,
            error: None,
        })
        .ok();
    }
    if remove_user_data {
        match fs::remove_dir_all(&app_data) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Não consegui remover os dados locais: {error}")),
        }
    }
    let applications = user_data_dir.join("applications");
    let _ = Command::new("update-desktop-database")
        .arg(&applications)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = Command::new("kbuildsycoca6")
        .arg("--noincremental")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = Command::new("gtk-update-icon-cache")
        .args(["--force", "--ignore-theme-index"])
        .arg(user_data_dir.join("icons/hicolor"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(())
}

fn install(
    restore_backup: Option<&Path>,
    create_desktop_shortcut: bool,
    send: &mpsc::Sender<Progress>,
) -> Result<(), String> {
    let home =
        PathBuf::from(env::var_os("HOME").ok_or("Não encontrei a pasta pessoal do usuário.")?);
    let restore_bytes = if let Some(path) = restore_backup {
        let bytes =
            fs::read(path).map_err(|e| format!("Não consegui ler o backup selecionado: {e}"))?;
        if bytes.len() > 128 * 1024 * 1024 {
            return Err("O backup excede o limite de 128 MB.".into());
        }
        let backup: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("O arquivo selecionado não é um JSON válido: {e}"))?;
        if backup.get("format").and_then(serde_json::Value::as_str) != Some("desire-vault-library")
            || backup.get("version").and_then(serde_json::Value::as_u64) != Some(1)
            || !backup.get("games").is_some_and(serde_json::Value::is_array)
        {
            return Err("Este não é um backup compatível do Desire Vault.".into());
        }
        Some(bytes)
    } else {
        None
    };
    let bin_dir = home.join(".local/bin");
    let user_data_dir = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let app_dir = user_data_dir.join("applications");
    let icon_dir = user_data_dir.join("icons/hicolor/256x256/apps");
    fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Não consegui preparar o destino do aplicativo: {e}"))?;
    fs::create_dir_all(&app_dir)
        .map_err(|e| format!("Não consegui preparar o menu de aplicativos: {e}"))?;
    fs::create_dir_all(&icon_dir)
        .map_err(|e| format!("Não consegui preparar o destino do ícone: {e}"))?;
    send.send(Progress {
        percent: 22.0,
        message: "Pastas de instalação preparadas".into(),
        done: false,
        error: None,
    })
    .ok();

    let binary = bin_dir.join("desire-vault");
    fs::write(&binary, APP_BINARY)
        .map_err(|e| format!("Não consegui instalar o aplicativo: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    send.send(Progress {
        percent: 58.0,
        message: "Aplicativo instalado".into(),
        done: false,
        error: None,
    })
    .ok();

    fs::write(icon_dir.join("desire-vault.png"), APP_ICON)
        .map_err(|e| format!("Não consegui instalar o ícone: {e}"))?;
    send.send(Progress {
        percent: 77.0,
        message: "Identidade visual instalada".into(),
        done: false,
        error: None,
    })
    .ok();

    let icon = icon_dir.join("desire-vault.png");
    let desktop = format!("[Desktop Entry]\nType=Application\nName=Desire Vault\nGenericName=Biblioteca de jogos\nComment=Organize sua biblioteca pessoal de jogos\nExec={}\nTryExec={}\nIcon={}\nTerminal=false\nStartupNotify=true\nDBusActivatable=false\nCategories=Game;\nKeywords=jogos;biblioteca;game;library;\n", binary.display(), binary.display(), icon.display());
    fs::write(app_dir.join("desire-vault.desktop"), &desktop)
        .map_err(|e| format!("Não consegui criar o atalho no menu: {e}"))?;
    if create_desktop_shortcut {
        let shortcut = desktop_dir(&home)?.join("Desire Vault.desktop");
        fs::write(&shortcut, &desktop)
            .map_err(|e| format!("Não consegui criar o atalho na área de trabalho: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&shortcut, fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
    }
    send.send(Progress {
        percent: 92.0,
        message: "Atalho do menu criado".into(),
        done: false,
        error: None,
    })
    .ok();
    if let Some(bytes) = restore_bytes {
        let data_dir = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("com.flavio.gamelibrarymanager");
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Não consegui preparar a restauração: {e}"))?;
        let pending = data_dir.join("pending-install-restore.json");
        fs::write(&pending, bytes)
            .map_err(|e| format!("Não consegui preparar o backup para restauração: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&pending, fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("Não consegui proteger o arquivo de backup: {e}"))?;
        }
        send.send(Progress {
            percent: 97.0,
            message: "Backup validado e preparado para a primeira abertura".into(),
            done: false,
            error: None,
        })
        .ok();
    }
    let _ = Command::new("update-desktop-database")
        .arg(&app_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = Command::new("kbuildsycoca6")
        .arg("--noincremental")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = Command::new("gtk-update-icon-cache")
        .args(["--force", "--ignore-theme-index"])
        .arg(user_data_dir.join("icons/hicolor"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(())
}

fn main() {
    let application =
        gtk::Application::new(Some("com.flavio.desirevault.installer"), Default::default());
    application.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::new(app);
        window.set_title("Desire Vault · Instalação");
        window.set_default_size(900, 600);
        window.set_size_request(760, 520);
        window.set_resizable(true);
        window.set_position(gtk::WindowPosition::Center);

        let icon_path = env::temp_dir().join(format!(
            "desire-vault-installer-icon-{}.png",
            std::process::id()
        ));
        let logo_path = env::temp_dir().join(format!(
            "desire-vault-installer-logo-{}.svg",
            std::process::id()
        ));
        let _ = fs::write(&icon_path, APP_ICON);
        let _ = fs::write(&logo_path, BRAND_LOGO);
        let _ = window.set_icon_from_file(&icon_path);

        let provider = gtk::CssProvider::new();
        let css = "
            window { background: #0d1119; color: #f5f1fa; font-family: Inter, 'Noto Sans', sans-serif; }
            .wizard { background: #11151e; }
            .sidebar { padding: 30px 24px; background: linear-gradient(160deg, #241936, #191728 52%, #171d2a); border-right: 1px solid rgba(255,255,255,0.08); }
            .sidebar-caption { color: #a8a0b8; font-size: 10px; font-weight: 700; letter-spacing: 1.5px; }
            .sidebar-step { color: #9995a4; font-size: 12px; padding: 8px 10px; border-radius: 8px; }
            .sidebar-step-active { color: #f4eaff; background: rgba(169,119,230,0.2); }
            .sidebar-foot { color: #aaa2b6; font-size: 11px; }
            .content-shell { padding: 34px 38px 20px; background: linear-gradient(145deg, #171c27, #11151e 70%, #181421); }
            .page-title { color: #faf7ff; font-size: 25px; font-weight: 700; }
            .page-desc { color: #b8b4c2; font-size: 13px; }
            .topbar { padding: 0 0 12px; border-bottom: 1px solid rgba(255,255,255,0.078); }
            .brand-chip { padding: 8px 12px; border: 1px solid rgba(184,124,255,0.220); border-radius: 12px; background: linear-gradient(135deg, #251b35, #171923); }
            .eyebrow { color: #c69be9; font-size: 10px; font-weight: 700; letter-spacing: 1.8px; }
            .hero { padding: 20px 22px; border: 1px solid rgba(159,112,219,0.3); border-radius: 14px; background: linear-gradient(120deg, #251d34, #1b1d29 58%, #18202a); }
            .hero-title { color: #fbf8ff; font-size: 27px; font-weight: 700; }
            .subtitle { color: #c0bacb; font-size: 13px; }
            .stepper { padding: 4px 3px; }
            .step { color: #c5c0cd; font-size: 11px; font-weight: 600; }
            .step-active { color: #d6b2f7; }
            .step-dot { color: #b17bec; font-size: 15px; font-weight: 700; }
            .panel { background: linear-gradient(145deg, #1e2430, #191e28); border: 1px solid #42404d; border-radius: 12px; padding: 17px 19px; }
            .panel-title { color: #f4effa; font-size: 14px; font-weight: 700; }
            .body-copy { color: #aaa8b5; font-size: 12px; }
            .status { color: #c4bfcb; font-size: 12px; }
            .status-error { color: #ff9d9d; }
            .status-success { color: #9be0b4; }
            .file-name { color: #d2b5f1; font-size: 11px; }
            .footer { color: #898d9a; font-size: 10px; }
            .nav-bar { padding-top: 12px; border-top: 1px solid rgba(255,255,255,0.09); }
            .info-strong { color: #eee7f7; font-size: 13px; font-weight: 700; }
            .info-detail { color: #aaa6b4; font-size: 12px; }
            .icon-badge { color: #d4b2f3; font-size: 21px; font-weight: 700; }
            progressbar trough { min-height: 12px; border-radius: 8px; background: #292d39; border: 1px solid rgba(255,255,255,0.071); }
            progressbar progress { min-height: 12px; border-radius: 8px; background: linear-gradient(90deg, #8d54d7, #c895ff); }
            checkbutton { color: #f0ebf6; font-size: 12px; font-weight: 600; }
            checkbutton check { min-width: 17px; min-height: 17px; border-radius: 5px; border: 1px solid #706580; background: #202530; }
            checkbutton:checked check { background: #a879e8; border-color: #d5b2ff; }
            button { min-height: 42px; border-radius: 10px; padding: 0 16px; background: linear-gradient(#303642, #272d37); color: #f1edf7; border: 1px solid rgba(255,255,255,0.125); font-weight: 600; }
            button:hover { background: linear-gradient(#3b3e4c, #2e3340); border-color: #c194f0; }
            button.suggested-action { min-height: 44px; border-radius: 9px; background: linear-gradient(100deg, #d2aeef, #b483e2); color: #191323; font-size: 13px; font-weight: 700; border: 1px solid #e3c8ff; }
            button.suggested-action:hover { background: linear-gradient(100deg, #e0c3f6, #c296ea); }
            button.danger-button { color: #f0b0b5; background: #32212a; border-color: #74434e; }
            button.danger-button:hover { background: #492932; border-color: #bd6874; }
            button:disabled { opacity: .55; }
        ";
        if let Err(error) = provider.load_from_data(css.as_bytes()) {
            eprintln!("Falha ao aplicar o tema visual do instalador: {error}");
        }
        if let Some(screen) = gtk::gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let wizard = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        wizard.style_context().add_class("wizard");
        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 18);
        sidebar.set_size_request(235, -1);
        sidebar.style_context().add_class("sidebar");
        let logo_pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(&logo_path, 200, 62, true).ok();
        let logo = gtk::Image::from_pixbuf(logo_pixbuf.as_ref());
        sidebar.pack_start(&logo, false, false, 0);
        let side_caption = gtk::Label::new(Some("ASSISTENTE DE CONFIGURAÇÃO"));
        side_caption.style_context().add_class("sidebar-caption");
        side_caption.set_xalign(0.0);
        sidebar.pack_start(&side_caption, false, false, 12);
        let steps = gtk::Box::new(gtk::Orientation::Vertical, 7);
        let mut step_labels = Vec::new();
        for text in ["1   Boas-vindas", "2   Opções", "3   Instalação", "4   Concluído"] {
            let label = gtk::Label::new(Some(text));
            label.set_xalign(0.0);
            label.style_context().add_class("sidebar-step");
            steps.pack_start(&label, false, false, 0);
            step_labels.push(label);
        }
        step_labels[0].style_context().add_class("sidebar-step-active");
        sidebar.pack_start(&steps, false, false, 0);
        let side_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        side_spacer.set_vexpand(true);
        sidebar.pack_start(&side_spacer, true, true, 0);
        let side_bottom = gtk::Label::new(Some("DESIRE VAULT\nSua biblioteca de jogos, organizada."));
        side_bottom.style_context().add_class("sidebar-foot");
        side_bottom.set_xalign(0.0);
        side_bottom.set_line_wrap(true);
        sidebar.pack_start(&side_bottom, false, false, 0);
        wizard.pack_start(&sidebar, false, false, 0);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_hexpand(true);
        content.style_context().add_class("content-shell");
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        stack.set_transition_duration(180);
        stack.set_vexpand(true);

        let home_for_detection = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let data_for_detection = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_for_detection.join(".local/share"));
        let installed_binary = home_for_detection.join(".local/bin/desire-vault");
        let installed_launcher = data_for_detection.join("applications/desire-vault.desktop");
        let already_installed = installed_binary.is_file() && installed_launcher.is_file();

        let welcome = gtk::Box::new(gtk::Orientation::Vertical, 17);
        let welcome_eyebrow = gtk::Label::new(Some(if already_installed {
            "DESIRE VAULT JÁ ESTÁ INSTALADO"
        } else {
            "INSTALAÇÃO DO APLICATIVO"
        }));
        welcome_eyebrow.style_context().add_class("eyebrow");
        welcome_eyebrow.set_xalign(0.0);
        welcome.pack_start(&welcome_eyebrow, false, false, 0);
        let welcome_title = gtk::Label::new(Some(if already_installed {
            "Desire Vault já está instalado"
        } else {
            "Bem-vindo ao Desire Vault"
        }));
        welcome_title.style_context().add_class("page-title");
        welcome_title.set_xalign(0.0);
        welcome.pack_start(&welcome_title, false, false, 0);
        let welcome_desc = gtk::Label::new(Some(if already_installed {
            "Uma cópia já existe neste usuário. Para evitar duplicação, uma nova instalação será bloqueada. Você pode desinstalar a cópia atual por aqui."
        } else {
            "Este assistente vai instalar o Desire Vault no seu usuário e criar um atalho no menu de aplicativos."
        }));
        welcome_desc.style_context().add_class("page-desc");
        welcome_desc.set_xalign(0.0);
        welcome_desc.set_line_wrap(true);
        welcome.pack_start(&welcome_desc, false, false, 0);
        let welcome_card = gtk::Box::new(gtk::Orientation::Vertical, 12);
        welcome_card.style_context().add_class("hero");
        for (strong, detail) in [
            ("Instalação simples", "Não precisa de senha de administrador."),
            ("Seus dados ficam locais", "A biblioteca permanece neste dispositivo."),
            ("Restaure sua biblioteca", "Você poderá escolher um backup na próxima etapa."),
        ] {
            let item = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            let mark = gtk::Label::new(Some("✓"));
            mark.style_context().add_class("icon-badge");
            item.pack_start(&mark, false, false, 0);
            let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
            let title = gtk::Label::new(Some(strong));
            title.style_context().add_class("info-strong");
            title.set_xalign(0.0);
            let copy = gtk::Label::new(Some(detail));
            copy.style_context().add_class("info-detail");
            copy.set_xalign(0.0);
            text.pack_start(&title, false, false, 0);
            text.pack_start(&copy, false, false, 0);
            item.pack_start(&text, true, true, 0);
            welcome_card.pack_start(&item, false, false, 0);
        }
        welcome.pack_start(&welcome_card, false, false, 5);
        let welcome_note = gtk::Label::new(Some(if already_installed {
            "Nenhum arquivo será alterado se você tentar instalar novamente."
        } else {
            "Tempo estimado: menos de um minuto"
        }));
        welcome_note.style_context().add_class("footer");
        welcome_note.set_xalign(0.0);
        welcome.pack_start(&welcome_note, false, false, 0);
        let uninstall_button = gtk::Button::with_label("Desinstalar Desire Vault…");
        uninstall_button.style_context().add_class("danger-button");
        uninstall_button.set_halign(gtk::Align::Start);
        uninstall_button.set_visible(already_installed);
        welcome.pack_start(&uninstall_button, false, false, 0);
        stack.add_named(&welcome, "welcome");

        let options = gtk::Box::new(gtk::Orientation::Vertical, 16);
        let options_title = gtk::Label::new(Some("Opções de instalação"));
        options_title.style_context().add_class("page-title");
        options_title.set_xalign(0.0);
        options.pack_start(&options_title, false, false, 0);
        let options_desc = gtk::Label::new(Some("Revise o destino e, se quiser, escolha uma biblioteca para restaurar."));
        options_desc.style_context().add_class("page-desc");
        options_desc.set_xalign(0.0);
        options_desc.set_line_wrap(true);
        options.pack_start(&options_desc, false, false, 0);
        let destination_panel = gtk::Box::new(gtk::Orientation::Vertical, 7);
        destination_panel.style_context().add_class("panel");
        let dest_title = gtk::Label::new(Some("Instalar para este usuário"));
        dest_title.style_context().add_class("panel-title");
        dest_title.set_xalign(0.0);
        destination_panel.pack_start(&dest_title, false, false, 0);
        let dest_desc = gtk::Label::new(Some("O aplicativo e o atalho serão adicionados à sua conta. Nenhuma senha de administrador é necessária."));
        dest_desc.style_context().add_class("body-copy");
        dest_desc.set_xalign(0.0);
        dest_desc.set_line_wrap(true);
        destination_panel.pack_start(&dest_desc, false, false, 0);
        options.pack_start(&destination_panel, false, false, 0);
        let desktop_shortcut_check = gtk::CheckButton::with_label("Criar atalho na área de trabalho");
        let desktop_shortcut_desc = gtk::Label::new(Some("Você também poderá abrir o app pelo menu de aplicativos."));
        desktop_shortcut_desc.style_context().add_class("body-copy");
        desktop_shortcut_desc.set_xalign(0.0);
        options.pack_start(&desktop_shortcut_check, false, false, 0);
        options.pack_start(&desktop_shortcut_desc, false, false, 0);

        let restore_panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
        restore_panel.style_context().add_class("panel");
        let restore_check = gtk::CheckButton::with_label("Restaurar uma biblioteca de backup");
        restore_panel.pack_start(&restore_check, false, false, 0);
        let restore_description = gtk::Label::new(Some("Opcional. Selecione um arquivo JSON exportado pelo Desire Vault. A restauração será preparada para a primeira abertura."));
        restore_description.style_context().add_class("body-copy");
        restore_description.set_xalign(0.0);
        restore_description.set_line_wrap(true);
        restore_panel.pack_start(&restore_description, false, false, 0);
        let restore_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let choose_backup = gtk::Button::with_label("Procurar backup…");
        let backup_label = gtk::Label::new(Some("Nenhum arquivo selecionado"));
        backup_label.style_context().add_class("file-name");
        backup_label.set_xalign(0.0);
        backup_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        restore_row.pack_start(&choose_backup, false, false, 0);
        restore_row.pack_start(&backup_label, true, true, 0);
        restore_panel.pack_start(&restore_row, false, false, 0);
        options.pack_start(&restore_panel, false, false, 0);
        let options_error = gtk::Label::new(Some(
            "Selecione um backup ou desmarque a opção de restauração.",
        ));
        options_error.style_context().add_class("status-error");
        options_error.set_xalign(0.0);
        options_error.set_no_show_all(true);
        options.pack_start(&options_error, false, false, 0);
        stack.add_named(&options, "options");

        let installation = gtk::Box::new(gtk::Orientation::Vertical, 18);
        let install_title = gtk::Label::new(Some("Instalando Desire Vault"));
        install_title.style_context().add_class("page-title");
        install_title.set_xalign(0.0);
        installation.pack_start(&install_title, false, false, 0);
        let status = gtk::Label::new(Some("Aguarde enquanto preparamos o aplicativo."));
        status.style_context().add_class("status");
        status.set_xalign(0.0);
        status.set_line_wrap(true);
        installation.pack_start(&status, false, false, 0);
        let progress = gtk::ProgressBar::new();
        progress.set_fraction(0.0);
        progress.set_show_text(true);
        progress.set_text(Some("Preparando…"));
        installation.pack_start(&progress, false, false, 0);
        let install_details = gtk::Label::new(Some("Copiando arquivos, configurando o ícone e criando o atalho no menu."));
        install_details.style_context().add_class("body-copy");
        install_details.set_xalign(0.0);
        install_details.set_line_wrap(true);
        installation.pack_start(&install_details, false, false, 0);
        stack.add_named(&installation, "install");

        let complete = gtk::Box::new(gtk::Orientation::Vertical, 18);
        let complete_mark = gtk::Label::new(Some("✓"));
        complete_mark.style_context().add_class("icon-badge");
        complete.pack_start(&complete_mark, false, false, 0);
        let complete_title = gtk::Label::new(Some("Instalação concluída"));
        complete_title.style_context().add_class("page-title");
        complete_title.set_xalign(0.0);
        complete.pack_start(&complete_title, false, false, 0);
        let complete_desc = gtk::Label::new(Some("Desire Vault está instalado. Você pode abri-lo pelo menu de aplicativos."));
        complete_desc.style_context().add_class("page-desc");
        complete_desc.set_xalign(0.0);
        complete_desc.set_line_wrap(true);
        complete.pack_start(&complete_desc, false, false, 0);
        let complete_card = gtk::Label::new(Some("Aplicativo instalado para sua conta\nAtalho adicionado ao menu de aplicativos"));
        complete_card.style_context().add_class("panel");
        complete_card.set_xalign(0.0);
        complete_card.set_line_wrap(true);
        complete.pack_start(&complete_card, false, false, 0);
        stack.add_named(&complete, "complete");
        content.pack_start(&stack, true, true, 0);

        let nav = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        nav.style_context().add_class("nav-bar");
        let nav_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        nav_spacer.set_hexpand(true);
        let cancel = gtk::Button::with_label("Cancelar");
        let back = gtk::Button::with_label("‹  Voltar");
        back.set_visible(false);
        let next = gtk::Button::with_label("Avançar  ›");
        next.style_context().add_class("suggested-action");
        nav.pack_start(&nav_spacer, true, true, 0);
        nav.pack_start(&cancel, false, false, 0);
        nav.pack_start(&back, false, false, 0);
        nav.pack_start(&next, false, false, 0);
        content.pack_end(&nav, false, false, 0);
        wizard.pack_start(&content, true, true, 0);
        window.add(&wizard);

        let restore_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let restore_path_for_choose = restore_path.clone();
        let backup_label_for_choose = backup_label.clone();
        let restore_check_for_choose = restore_check.clone();
        let parent = window.clone();
        choose_backup.connect_clicked(move |_| {
            restore_check_for_choose.set_active(true);
            let chooser = gtk::FileChooserDialog::builder()
                .title("Escolher backup do Desire Vault")
                .transient_for(&parent)
                .action(gtk::FileChooserAction::Open)
                .modal(true)
                .build();
            chooser.add_button("Cancelar", gtk::ResponseType::Cancel);
            chooser.add_button("Selecionar backup", gtk::ResponseType::Accept);
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Backups JSON (*.json)"));
            filter.add_pattern("*.json");
            chooser.add_filter(filter);
            chooser.set_select_multiple(false);
            if chooser.run() == gtk::ResponseType::Accept {
                if let Some(path) = chooser.filename() {
                    backup_label_for_choose
                        .set_text(&path.file_name().unwrap_or_default().to_string_lossy());
                    *restore_path_for_choose.borrow_mut() = Some(path);
                }
            } else if restore_path_for_choose.borrow().is_none() {
                restore_check_for_choose.set_active(false);
            }
            chooser.hide();
        });

        let current_step = Rc::new(Cell::new(0u8));
        let update_step = {
            let labels = step_labels.iter().cloned().collect::<Vec<_>>();
            Rc::new(move |active: usize| {
                for (index, label) in labels.iter().enumerate() {
                    if index == active { label.style_context().add_class("sidebar-step-active"); }
                    else { label.style_context().remove_class("sidebar-step-active"); }
                }
            })
        };
        let run_install: Rc<dyn Fn()> = {
            let status = status.clone();
            let progress = progress.clone();
            let next = next.clone();
            let restore_check = restore_check.clone();
            let restore_path = restore_path.clone();
            let desktop_shortcut_check = desktop_shortcut_check.clone();
            let stack = stack.clone();
            let update_step = update_step.clone();
            let current_step = current_step.clone();
            let complete_desc = complete_desc.clone();
            Rc::new(move || {
                status.style_context().remove_class("status-error");
                status.style_context().remove_class("status-success");
                status.set_text("Preparando o aplicativo e o atalho do menu…");
                progress.set_fraction(0.02);
                progress.set_text(Some("Iniciando…"));
                next.set_sensitive(false);
                let restore_backup = if restore_check.is_active() { restore_path.borrow().clone() } else { None };
                let create_desktop_shortcut = desktop_shortcut_check.is_active();
                let (sender, receiver) = mpsc::channel::<Progress>();
                thread::spawn(move || {
                    let result = install(restore_backup.as_deref(), create_desktop_shortcut, &sender);
                    let final_update = match result {
                        Ok(()) => Progress {
                            percent: 100.0,
                            message: match (restore_backup.is_some(), create_desktop_shortcut) {
                                (true, true) => "Instalação concluída. O backup será restaurado ao abrir o app; os atalhos estão no menu e na área de trabalho.".into(),
                                (true, false) => "Instalação concluída. O backup será restaurado ao abrir o app; o atalho está no menu de aplicativos.".into(),
                                (false, true) => "Instalação concluída! Os atalhos estão no menu e na área de trabalho.".into(),
                                (false, false) => "Instalação concluída! Procure Desire Vault no menu de aplicativos.".into(),
                            },
                            done: true, error: None,
                        },
                        Err(error) => Progress { percent: 0.0, message: "A instalação não foi concluída.".into(), done: true, error: Some(error) },
                    };
                    sender.send(final_update).ok();
                });
                let status = status.clone();
                let progress = progress.clone();
                let next = next.clone();
                let stack = stack.clone();
                let update_step = update_step.clone();
                let current_step = current_step.clone();
                let complete_desc = complete_desc.clone();
                gtk::glib::timeout_add_local(Duration::from_millis(80), move || {
                    for update in receiver.try_iter() {
                        status.set_text(update.error.as_deref().unwrap_or(&update.message));
                        progress.set_fraction(update.percent / 100.0);
                        progress.set_text(Some(&format!("{:.0}%", update.percent)));
                        if update.done {
                            next.set_sensitive(true);
                            if update.error.is_some() {
                                status.style_context().add_class("status-error");
                                next.set_label("Tentar novamente");
                            } else {
                                status.style_context().add_class("status-success");
                                complete_desc.set_text(&update.message);
                                stack.set_visible_child_name("complete");
                                current_step.set(3);
                                update_step(3);
                                next.set_label("Concluir");
                                next.style_context().add_class("suggested-action");
                            }
                            return gtk::glib::ControlFlow::Break;
                        }
                    }
                    gtk::glib::ControlFlow::Continue
                });
            })
        };

        let window_for_cancel = window.clone();
        cancel.connect_clicked(move |_| window_for_cancel.close());
        let stack_for_back = stack.clone();
        let back_step = current_step.clone();
        let back_update = update_step.clone();
        let back_next = next.clone();
        let back_button = back.clone();
        back.connect_clicked(move |_| {
            stack_for_back.set_visible_child_name("welcome");
            back_step.set(0);
            back_update(0);
            back_next.set_label("Avançar  ›");
            back_button.set_visible(false);
        });
        let stack_for_next = stack.clone();
        let step_for_next = current_step.clone();
        let back_for_next = back.clone();
        let cancel_for_next = cancel.clone();
        let update_for_next = update_step.clone();
        let restore_check_for_next = restore_check.clone();
        let restore_path_for_next = restore_path.clone();
        let options_error_for_next = options_error.clone();
        let run_for_next = run_install.clone();
        let window_for_next = window.clone();
        let already_installed_for_next = already_installed;
        next.connect_clicked(move |button| match step_for_next.get() {
            0 => {
                if already_installed_for_next {
                    let warning = gtk::MessageDialog::new(
                        Some(&window_for_next),
                        gtk::DialogFlags::MODAL,
                        gtk::MessageType::Warning,
                        gtk::ButtonsType::Ok,
                        "O Desire Vault já está instalado para este usuário. A instalação duplicada foi bloqueada; nenhum arquivo foi alterado.",
                    );
                    warning.set_title("Aplicativo já instalado");
                    warning.run();
                    warning.hide();
                } else {
                    stack_for_next.set_visible_child_name("options");
                    step_for_next.set(1);
                    update_for_next(1);
                    back_for_next.set_visible(true);
                    button.set_label("Instalar");
                }
            }
            1 => {
                if restore_check_for_next.is_active() && restore_path_for_next.borrow().is_none() {
                    options_error_for_next.show();
                    return;
                }
                options_error_for_next.hide();
                stack_for_next.set_visible_child_name("install");
                step_for_next.set(2);
                update_for_next(2);
                back_for_next.set_visible(false);
                cancel_for_next.set_sensitive(false);
                button.set_label("Instalando…");
                run_for_next();
            }
            2 if button.label().as_deref() == Some("Tentar novamente") => {
                button.set_label("Instalando…");
                run_for_next();
            }
            3 => window_for_next.close(),
            _ => {}
        });
        let parent_for_uninstall = window.clone();
        uninstall_button.connect_clicked(move |_| {
            let confirm = gtk::MessageDialog::new(
                Some(&parent_for_uninstall),
                gtk::DialogFlags::MODAL,
                gtk::MessageType::Warning,
                gtk::ButtonsType::None,
                "O aplicativo, o atalho do menu, o ícone e o atalho da área de trabalho serão removidos.",
            );
            confirm.set_title("Desinstalar Desire Vault");
            let delete_data = gtk::CheckButton::with_label("Apagar também a biblioteca e os dados locais deste usuário");
            confirm.content_area().pack_start(&delete_data, false, false, 10);
            confirm.add_button("Manter instalado", gtk::ResponseType::Cancel);
            confirm.add_button("Desinstalar aplicativo", gtk::ResponseType::Accept);
            confirm.show_all();
            let response = confirm.run();
            confirm.hide();
            if response == gtk::ResponseType::Accept {
                let (sender, _receiver) = mpsc::channel::<Progress>();
                match uninstall(delete_data.is_active(), &sender) {
                    Ok(()) => {
                        let done = gtk::MessageDialog::new(
                            Some(&parent_for_uninstall), gtk::DialogFlags::MODAL,
                            gtk::MessageType::Info, gtk::ButtonsType::Ok,
                            "Desire Vault e seus atalhos foram removidos deste usuário.",
                        );
                        done.set_title("Desinstalação concluída");
                        done.run();
                        done.hide();
                        parent_for_uninstall.close();
                    }
                    Err(error) => {
                        let failed = gtk::MessageDialog::new(
                            Some(&parent_for_uninstall), gtk::DialogFlags::MODAL,
                            gtk::MessageType::Error, gtk::ButtonsType::Close, &error,
                        );
                        failed.set_title("Não foi possível concluir a desinstalação");
                        failed.run();
                        failed.hide();
                    }
                }
            }
        });
        let _ = fs::remove_file(&logo_path);
        let _ = fs::remove_file(&icon_path);
        window.show_all();
        back.hide();
    });
    application.run();
}
