// Prüfstand — nur im Dev-Build. Nimmt Befehle über die Standardeingabe an, damit sich die Shell auf dem
// Test-Mac per SSH prüfen lässt (dort gibt es weder Maus noch Bildschirmfoto). Eine Zeile je Befehl:
//
//   fenster                 alle Fenster der Shell: sichtbar, Fokus, Lage, Größe, Adresse
//   js <code>               JavaScript im Hauptfenster ausführen, Ergebnis ausgeben
//   js@<fenster> <code>     dasselbe in einem anderen Fenster (menueleiste, ablauf, schwebe)
//   menueleiste             wie ein Klick aufs Menüleisten-Symbol
//   schwebe-auf | schwebe-zu
//   nach-vorn               Hauptfenster nach vorn holen
//
// Start z. B.:  mkfifo steuer; sleep 99999 > steuer &  ./prepwell-desktop < steuer   — danach: echo fenster > steuer

use std::io::BufRead;

use tauri::{AppHandle, Manager};

use crate::{bruecke, haupt, menueleiste, schwebe};

const LOG: &str = "[Prüfstand]";

pub fn starte(app: &AppHandle) {
  let app = app.clone();
  std::thread::spawn(move || {
    for zeile in std::io::stdin().lock().lines().map_while(Result::ok) {
      fuehre_aus(&app, zeile.trim());
    }
  });
}

fn fuehre_aus(app: &AppHandle, zeile: &str) {
  let (befehl, rest) = zeile.split_once(' ').unwrap_or((zeile, ""));
  match befehl {
    "" => {}
    "fenster" => {
      let mut labels: Vec<_> = app.webview_windows().into_iter().collect();
      labels.sort_by(|a, b| a.0.cmp(&b.0));
      for (label, fenster) in labels {
        println!(
          "{LOG} {label}: sichtbar={:?} fokus={:?} ort={:?} groesse={:?} url={}",
          fenster.is_visible().ok(),
          fenster.is_focused().ok(),
          fenster.outer_position().ok().map(|o| (o.x, o.y)),
          fenster.inner_size().ok().map(|g| (g.width, g.height)),
          fenster.url().map(|u| u.to_string()).unwrap_or_default(),
        );
      }
      let tray = app.tray_by_id("timer").and_then(|t| t.rect().ok().flatten());
      println!("{LOG} menueleisten-symbol: titel={:?} lage={tray:?}", menueleiste::titel_jetzt());
    }
    // Tastenkürzel eines Menüeintrags, z. B. `kuerzel Vergrößern`.
    #[cfg(target_os = "macos")]
    "kuerzel" => {
      let titel = rest.to_owned();
      let _ = app.run_on_main_thread(move || println!("{LOG} kuerzel {titel}: {:?}", crate::mac::kuerzel_von(&titel)));
    }
    "menueleiste" => {
      let handle = app.clone();
      let _ = app.run_on_main_thread(move || menueleiste::umschalten(&handle, None));
    }
    "schwebe-auf" => {
      tauri::async_runtime::spawn(schwebe::oeffnen(app.clone(), true));
    }
    "schwebe-zu" => {
      let handle = app.clone();
      let _ = app.run_on_main_thread(move || schwebe::schliessen(&handle));
    }
    "nach-vorn" => {
      let handle = app.clone();
      let _ = app.run_on_main_thread(move || haupt::hole_app_nach_vorn(&handle));
    }
    _ if befehl == "js" || befehl.starts_with("js@") => {
      let label = befehl.strip_prefix("js@").unwrap_or(haupt::LABEL).to_owned();
      let Some(fenster) = app.get_webview_window(&label) else {
        println!("{LOG} kein Fenster „{label}“");
        return;
      };
      let code = rest.to_owned();
      tauri::async_runtime::spawn(async move {
        let ergebnis = bruecke::werte_aus_in(&fenster, code).await;
        println!("{LOG} js@{label} → {}", ergebnis.map_or("(keine Antwort)".into(), |wert| wert.to_string()));
      });
    }
    _ => println!("{LOG} unbekannt: {zeile}"),
  }
}
