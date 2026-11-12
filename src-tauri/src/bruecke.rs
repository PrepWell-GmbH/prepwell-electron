// Brücke zum Timer der Web-App (Frontend: features/timer/hooks/use-desktop-timer.ts, D-46/D-47, und use-floating-timer.ts, ADR-0070).
// Liest sekündlich `window.__prepwellDesktopTimer.getStand()` und den Schwebe-Timer-Hook aus dem Hauptfenster und reicht Befehle an `steuern()` weiter.
// Fehlt der Hook (ältere Web-App, nicht eingeloggt, Seite lädt), gilt "kein Timer" — die Hülle bleibt still.

use std::{sync::Mutex, time::Duration};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, WebviewWindow};

use crate::{ablauf, haupt, menueleiste, schwebe};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Ablauf {
  pub nr: f64,
  /// Zeitpunkt des Ablaufs (epoch ms) — Kennung eines Ablaufs, auch über ein Neuladen der Web-App hinweg.
  pub um: f64,
  /// 'pause' | 'fokus' | 'fertig'
  pub art: String,
  pub titel: String,
  pub text: String,
  pub wartet: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TimerStand {
  pub aktiv: bool,
  pub pausiert: bool,
  /// 'pomodoro' | 'countdown' | 'stoppuhr'
  pub modus: Option<String>,
  pub pause: bool,
  pub text: Option<String>,
  pub label: Option<String>,
  pub fortschritt: f64,
  pub ablauf: Option<Ablauf>,
}

/// Was der Schwebe-Timer anzeigt (Hook `__prepwellFloatingTimer`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchwebeStand {
  pub hook_present: bool,
  pub active: bool,
  /// Anzeige-Text vom Frontend (z. B. "24:31"), sonst None.
  pub text: Option<String>,
  pub label: Option<String>,
}

/// Eine Lesung aus dem Hauptfenster.
#[derive(Clone, Debug, Default)]
pub struct Lesung {
  pub stand: TimerStand,
  pub schwebe: SchwebeStand,
}

const BEFEHLE: [&str; 7] = ["pause", "weiter", "beenden", "start-pomodoro", "start-countdown", "start-stoppuhr", "timer-oeffnen"];
const TAKT: Duration = Duration::from_secs(1);
/// So lange darf die Web-App für eine Antwort brauchen; danach gilt "kein Timer".
const ANTWORTFRIST: Duration = Duration::from_millis(800);

// Läuft im Hauptfenster. Tolerant gegenüber einem fehlenden Hook und gegenüber dem älteren
// Schwebe-Hook, der nur isTimerActive() kennt. Das Ergebnis geht als JSON-Text zurück.
const PROBE: &str = r#"(() => {
  let stand = null;
  try {
    const hook = window.__prepwellDesktopTimer;
    if (hook && typeof hook.getStand === 'function') stand = hook.getStand();
  } catch (e) { stand = null; }

  const schwebe = { hookPresent: false, active: false, text: null, label: null };
  try {
    const hook = window.__prepwellFloatingTimer;
    if (hook) {
      schwebe.hookPresent = true;
      try { schwebe.active = !!hook.isTimerActive(); } catch (e) {}
      if (typeof hook.getDisplayState === 'function') {
        const s = hook.getDisplayState();
        if (s) {
          schwebe.text = typeof s.text === 'string' ? s.text : null;
          schwebe.label = typeof s.label === 'string' ? s.label : null;
          if (typeof s.active === 'boolean') schwebe.active = s.active;
        }
      }
    }
  } catch (e) {}

  try { return JSON.stringify({ stand, schwebe }); } catch (e) { return 'null'; }
})()"#;

/// Führt JavaScript im Hauptfenster aus und liefert das Ergebnis als JSON-Wert — None, wenn es kein Hauptfenster gibt oder die Seite nicht antwortet.
async fn werte_aus(app: &AppHandle, js: String) -> Option<Value> {
  werte_aus_in(&haupt::fenster(app)?, js).await
}

/// Wie `werte_aus`, für ein beliebiges Fenster.
pub async fn werte_aus_in(fenster: &WebviewWindow, js: String) -> Option<Value> {
  let (sender, empfaenger) = tokio::sync::oneshot::channel::<String>();
  let sender = Mutex::new(Some(sender));
  fenster
    .eval_with_callback(js, move |ergebnis| {
      if let Some(sender) = sender.lock().unwrap().take() {
        let _ = sender.send(ergebnis);
      }
    })
    .ok()?;
  let roh = tokio::time::timeout(ANTWORTFRIST, empfaenger).await.ok()?.ok()?;
  let wert: Value = serde_json::from_str(&roh).ok()?;
  // Das Skript liefert JSON-Text; WebKit verpackt ihn noch einmal als JSON-Zeichenkette.
  match wert {
    Value::String(text) => Some(serde_json::from_str(&text).unwrap_or(Value::String(text))),
    anderes => Some(anderes),
  }
}

fn text(wert: &Value, feld: &str) -> Option<String> {
  wert.get(feld).and_then(Value::as_str).map(str::to_owned)
}

fn ja(wert: &Value, feld: &str) -> bool {
  wert.get(feld).and_then(Value::as_bool) == Some(true)
}

/// Prüft, was aus der Web-App kommt — fremde Eingabe.
fn als_stand(roh: &Value) -> TimerStand {
  let Some(aktiv) = roh.get("aktiv").and_then(Value::as_bool) else {
    return TimerStand::default();
  };
  let ablauf = roh.get("ablauf").and_then(|a| {
    Some(Ablauf {
      nr: a.get("nr")?.as_f64()?,
      um: a.get("um")?.as_f64()?,
      art: text(a, "art").unwrap_or_else(|| "fertig".into()),
      titel: text(a, "titel")?,
      text: text(a, "text")?,
      wartet: ja(a, "wartet"),
    })
  });
  TimerStand {
    aktiv,
    pausiert: ja(roh, "pausiert"),
    modus: text(roh, "modus"),
    pause: ja(roh, "pause"),
    text: text(roh, "text"),
    label: text(roh, "label"),
    fortschritt: roh.get("fortschritt").and_then(Value::as_f64).map_or(0.0, |f| f.clamp(0.0, 1.0)),
    ablauf,
  }
}

fn als_schwebe(roh: &Value) -> SchwebeStand {
  SchwebeStand {
    hook_present: ja(roh, "hookPresent"),
    active: ja(roh, "active"),
    text: text(roh, "text"),
    label: text(roh, "label"),
  }
}

/// Liest den Timer aus dem Hauptfenster. Ohne Fenster oder ohne Antwort: "kein Timer".
pub async fn lesen(app: &AppHandle) -> Lesung {
  let Some(wert) = werte_aus(app, PROBE.into()).await else {
    return Lesung::default();
  };
  Lesung {
    stand: wert.get("stand").map(als_stand).unwrap_or_default(),
    schwebe: wert.get("schwebe").map(als_schwebe).unwrap_or_default(),
  }
}

/// Schickt einen Befehl an die Web-App und liefert 'ok' | 'app' | 'ungueltig'. Unbekannte Befehle verlassen die Hülle nicht.
pub async fn sende_befehl(app: &AppHandle, befehl: &str) -> &'static str {
  if !BEFEHLE.contains(&befehl) {
    return "ungueltig";
  }
  let js = format!(
    "(() => {{ try {{ const h = window.__prepwellDesktopTimer; return JSON.stringify(h && typeof h.steuern === 'function' ? h.steuern({}) : 'ungueltig'); }} catch (e) {{ return '\"ungueltig\"'; }} }})()",
    Value::String(befehl.into())
  );
  match werte_aus(app, js).await.as_ref().and_then(Value::as_str) {
    Some("ok") => "ok",
    Some("app") => "app",
    _ => "ungueltig",
  }
}

/// Sofort einmal lesen und verteilen (z. B. direkt nach einem Befehl).
pub async fn jetzt_lesen(app: &AppHandle) {
  let lesung = lesen(app).await;
  menueleiste::auf_stand(app, &lesung.stand);
  ablauf::auf_stand(app, &lesung.stand);
  schwebe::auf_takt(app, &lesung.schwebe);
}

/// Startet den Sekundentakt. Läuft für die ganze Lebenszeit der App; ohne Hauptfenster liefert er "kein Timer".
pub fn starte(app: &AppHandle) {
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    let mut takt = tokio::time::interval(TAKT);
    takt.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
      takt.tick().await;
      jetzt_lesen(&app).await;
    }
  });
}
