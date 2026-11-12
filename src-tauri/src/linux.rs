// Linux-Entsprechungen zu mac.rs. Verteilt wird die Shell nur für macOS; diese Fassung ist für den eigenen Rechner.
// Was fehlt: Symbol und Restzeit in der Menüleiste, Auto-Update, Fenster über Vollbild-Apps.

use tauri::WebviewWindow;

// Fensterebenen kennt nur macOS; „immer im Vordergrund" setzt schon fenster.rs.
pub const EBENE_MENUE: isize = 0;
pub const EBENE_GANZ_OBEN: isize = 0;

/// Läuft vor dem Start von GTK.
pub fn vor_dem_start() {
  // Unter Wayland darf ein Programm seine Fenster weder selbst platzieren noch über anderen halten —
  // Schwebe-Timer und Ablauf-Fenster brauchen beides. Über XWayland geht es. Wer GDK_BACKEND selbst setzt, behält recht.
  if std::env::var_os("GDK_BACKEND").is_none() {
    std::env::set_var("GDK_BACKEND", "x11");
  }
}

pub fn als_panel(_fenster: &WebviewWindow, _ebene: isize) {}

/// Zeigen, ohne der App im Vordergrund den Fokus zu nehmen (fenster.rs erzeugt die Fenster mit `focused(false)`).
pub fn zeige_inaktiv(fenster: &WebviewWindow) {
  let _ = fenster.show();
  let _ = fenster.set_always_on_top(true);
}

pub fn zeige_mit_fokus(fenster: &WebviewWindow) {
  let _ = fenster.show();
  let _ = fenster.set_focus();
}

/// Nur macOS braucht das (siehe mac.rs); unter Linux gilt das Kürzel aus haupt.rs.
pub fn kuerzel_nach_zeichen(_titel: &str, _zeichen: &str) {}
