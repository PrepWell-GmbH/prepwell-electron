// macOS-Eigenes, das Tauri nicht anbietet: die kleinen Timer-Fenster als NSPanel.
//
// Warum NSPanel: Ein gewöhnliches Fenster einer normalen App darf seit macOS 10.14 nicht über den
// Vollbild-Apps anderer Programme liegen. Ein nicht-aktivierendes Panel darf das — und es lässt sich
// zeigen und anklicken, ohne dass PrepWell der App, in der der Nutzer gerade arbeitet, den Fokus nimmt.
// (Electron löste dasselbe, indem es das Dock-Symbol der App kurz ausblendete.)
//
// Tauri erzeugt nur gewöhnliche Fenster. Deshalb wird hier die Klasse des fertigen Fensters getauscht.
// Was dabei schiefgehen kann, ist am Test-Mac vermessen (macOS 26.5) und unten je Schritt vermerkt.
//
// WICHTIG: Ein umgewandeltes Fenster wird nie zerstört, nur aus- und eingeblendet. Nach `destroy()` gibt macOS
// ein solches Fenster nicht mehr frei (gemessen: sechsmal öffnen und zerstören hinterlässt sechs tote Fenster).

use std::ffi::{c_char, CStr};

use objc2::{
  define_class, msg_send,
  rc::Retained,
  runtime::AnyObject,
  sel, ClassType, MainThreadOnly,
};
use objc2_app_kit::{NSPanel, NSWindow};
use tauri::WebviewWindow;

/// Fenster-Ebene über allem, auch über Vollbild-Apps (kCGScreenSaverWindowLevel).
pub const EBENE_GANZ_OBEN: isize = 1000;
/// Ebene eines aufgeklappten Menüs (NSPopUpMenuWindowLevel).
pub const EBENE_MENUE: isize = 101;

const AUF_ALLEN_SPACES: usize = 1 << 0; // NSWindowCollectionBehaviorCanJoinAllSpaces
const NEBEN_VOLLBILD: usize = 1 << 8; // NSWindowCollectionBehaviorFullScreenAuxiliary

define_class!(
  // Ein rahmenloses Panel nimmt von Haus aus keine Tastatur an; das Menüleisten-Fenster braucht sie,
  // um zu merken, wann daneben geklickt wird.
  #[unsafe(super(NSPanel))]
  #[thread_kind = MainThreadOnly]
  #[name = "PrepWellPanel"]
  struct PrepWellPanel;

  impl PrepWellPanel {
    #[unsafe(method(canBecomeKeyWindow))]
    fn can_become_key_window(&self) -> bool {
      true
    }

    #[unsafe(method(canBecomeMainWindow))]
    fn can_become_main_window(&self) -> bool {
      false
    }
  }
);

/// Führt `tu` mit dem NSWindow des Fensters auf dem Haupt-Thread aus (AppKit erlaubt nichts anderes).
fn mit_ns_window(fenster: &WebviewWindow, tu: impl FnOnce(&AnyObject) + Send + 'static) {
  let ziel = fenster.clone();
  let _ = fenster.run_on_main_thread(move || {
    let Ok(zeiger) = ziel.ns_window() else { return };
    if zeiger.is_null() {
      return;
    }
    // SAFETY: `ns_window()` liefert das lebende NSWindow dieses Fensters; wir sind auf dem Haupt-Thread.
    tu(unsafe { &*(zeiger as *const AnyObject) });
  });
}

/// Text eines Objective-C-Objekts (`-description`).
unsafe fn beschreibung(objekt: &AnyObject) -> String {
  let text: Option<Retained<AnyObject>> = msg_send![objekt, description];
  let Some(text) = text else { return String::new() };
  let zeichen: *const c_char = msg_send![&*text, UTF8String];
  if zeichen.is_null() {
    return String::new();
  }
  CStr::from_ptr(zeichen).to_string_lossy().into_owned()
}

/// Die Schlüsselpfade, auf denen das Fenster gerade per KVO beobachtet wird.
unsafe fn beobachtete_pfade(ns_window: &AnyObject) -> Vec<String> {
  let info: *mut AnyObject = msg_send![ns_window, observationInfo];
  if info.is_null() {
    return Vec::new();
  }
  // observationInfo zeigt auf ein Objekt, das seine Beobachtungen nur als Text preisgibt.
  beschreibung(&*info)
    .split("Key path: ")
    .skip(1)
    .map(|rest| rest.split(',').next().unwrap_or_default().trim().to_owned())
    .collect()
}

/// Tauscht die Klasse des Fensters gegen das Panel. Liefert false, wenn das nicht gefahrlos geht.
///
/// Drei Dinge machen den Tausch am lebenden Objekt sicher:
/// - NSPanel bringt keine eigenen Instanzvariablen über NSWindow hinaus mit (sonst läge es über dem Speicher des Fensters).
/// - Niemand hängt mit einer KVO-Beobachtung an der alten Klasse. WebKit beobachtet das Fenster, solange die
///   Webansicht darin hängt, und die App stürzt beim Schließen ab, wenn die Klasse dazwischen getauscht wurde —
///   deshalb wird die Webansicht für den Tausch kurz ausgehängt und meldet sich danach an der neuen Klasse neu an.
///   Übrig bleiben darf nur AppKits eigene Beobachtung `_windowLayerContext`: sie hängt an setOpaque:/setContentView:,
///   und beides ruft nach dem Tausch niemand mehr. Beobachtet sonst noch jemand, bleibt das Fenster, wie es ist.
/// - „Nicht aktivierend" wird über AppKits Schalter `_setPreventsActivation:` gesetzt, NICHT über die Fenstermaske:
///   ein nachträgliches setStyleMask: lässt das Fenster nach dem Schließen als Geist auf dem Bildschirm stehen.
unsafe fn tausche_klasse(ns_window: &AnyObject) -> bool {
  if NSPanel::class().instance_size() != NSWindow::class().instance_size() {
    return false;
  }
  let kennt_schalter: bool = msg_send![ns_window, respondsToSelector: sel!(_setPreventsActivation:)];
  if !kennt_schalter {
    return false;
  }
  let inhalt: Option<Retained<AnyObject>> = msg_send![ns_window, contentView];
  let Some(inhalt) = inhalt else { return false };
  let liste: Option<Retained<AnyObject>> = msg_send![&*inhalt, subviews];
  let ansichten: Vec<Retained<AnyObject>> = match liste {
    Some(liste) => {
      let anzahl: usize = msg_send![&*liste, count];
      (0..anzahl).filter_map(|i| msg_send![&*liste, objectAtIndex: i]).collect()
    }
    None => Vec::new(),
  };

  for ansicht in &ansichten {
    let _: () = msg_send![&**ansicht, removeFromSuperview];
  }
  let getauscht = beobachtete_pfade(ns_window).iter().all(|pfad| pfad == "_windowLayerContext");
  if getauscht {
    AnyObject::set_class(ns_window, PrepWellPanel::class());
    let _: () = msg_send![ns_window, _setPreventsActivation: true];
  }
  for ansicht in &ansichten {
    let _: () = msg_send![&*inhalt, addSubview: &**ansicht];
  }
  getauscht
}

/// Macht aus dem Fenster ein nicht-aktivierendes Panel auf der Ebene `ebene`, sichtbar auf allen Spaces und über Vollbild-Apps.
/// Gelingt die Umwandlung nicht, bleibt es ein gewöhnliches Fenster auf derselben Ebene — es funktioniert dann weiter,
/// liegt aber womöglich nicht über Vollbild-Apps.
pub fn als_panel(fenster: &WebviewWindow, ebene: isize) {
  mit_ns_window(fenster, move |ns_window| unsafe {
    if !tausche_klasse(ns_window) {
      eprintln!("[Fenster] Panel-Umwandlung nicht möglich — Timer-Fenster bleibt ein gewöhnliches Fenster");
    }
    let _: () = msg_send![ns_window, setLevel: ebene];
    let _: () = msg_send![ns_window, setCollectionBehavior: AUF_ALLEN_SPACES | NEBEN_VOLLBILD];
    let _: () = msg_send![ns_window, setHidesOnDeactivate: false];
  });
}

/// Zeigt das Fenster, ohne der App im Vordergrund den Fokus zu nehmen (Electrons showInactive).
pub fn zeige_inaktiv(fenster: &WebviewWindow) {
  mit_ns_window(fenster, |ns_window| unsafe {
    let _: () = msg_send![ns_window, orderFrontRegardless];
  });
}

/// Zeigt das Fenster und gibt ihm die Tastatur — ohne PrepWell als Ganzes nach vorn zu holen.
pub fn zeige_mit_fokus(fenster: &WebviewWindow) {
  mit_ns_window(fenster, |ns_window| unsafe {
    let _: () = msg_send![ns_window, orderFrontRegardless];
    let _: () = msg_send![ns_window, makeKeyWindow];
  });
}

/// Setzt das Tastenkürzel eines Menüeintrags nach dem ZEICHEN statt nach der Tastenposition: ⌘ + `zeichen`.
/// Tauri kennt nur Tastenpositionen der US-Tastatur — „Vergrößern" läge dort auf ⌘=, das ist auf der deutschen
/// Tastatur ⌘⇧0. Mit dem Zeichen „+" gilt die Plus-Taste jeder Belegung (wie in Safari und Chrome).
/// Muss auf dem Hauptthread laufen, nachdem die Menüzeile gesetzt ist.
pub fn kuerzel_nach_zeichen(titel: &str, zeichen: &str) {
  const BEFEHLSTASTE: usize = 1 << 20; // NSEventModifierFlagCommand
  let (Ok(titel), Ok(zeichen)) = (std::ffi::CString::new(titel), std::ffi::CString::new(zeichen)) else { return };
  unsafe {
    let ns_string = |text: &CStr| -> *mut AnyObject { msg_send![objc2::class!(NSString), stringWithUTF8String: text.as_ptr()] };
    let app: *mut AnyObject = msg_send![objc2::class!(NSApplication), sharedApplication];
    let menue: *mut AnyObject = msg_send![app, mainMenu];
    if menue.is_null() {
      return;
    }
    let anzahl: isize = msg_send![menue, numberOfItems];
    for stelle in 0..anzahl {
      let oben: *mut AnyObject = msg_send![menue, itemAtIndex: stelle];
      let unter: *mut AnyObject = msg_send![oben, submenu];
      if unter.is_null() {
        continue;
      }
      let eintrag: *mut AnyObject = msg_send![unter, itemWithTitle: ns_string(&titel)];
      if !eintrag.is_null() {
        let _: () = msg_send![eintrag, setKeyEquivalent: ns_string(&zeichen)];
        let _: () = msg_send![eintrag, setKeyEquivalentModifierMask: BEFEHLSTASTE];
        return;
      }
    }
    eprintln!("[Menü] Eintrag für das Tastenkürzel nicht gefunden");
  }
}

/// Liest das Tastenkürzel eines Menüeintrags zurück (für den Prüfstand).
#[cfg(debug_assertions)]
pub fn kuerzel_von(titel: &str) -> Option<String> {
  let titel = std::ffi::CString::new(titel).ok()?;
  unsafe {
    let app: *mut AnyObject = msg_send![objc2::class!(NSApplication), sharedApplication];
    let menue: *mut AnyObject = msg_send![app, mainMenu];
    if menue.is_null() {
      return None;
    }
    let ns_titel: *mut AnyObject = msg_send![objc2::class!(NSString), stringWithUTF8String: titel.as_ptr()];
    let anzahl: isize = msg_send![menue, numberOfItems];
    for stelle in 0..anzahl {
      let oben: *mut AnyObject = msg_send![menue, itemAtIndex: stelle];
      let unter: *mut AnyObject = msg_send![oben, submenu];
      if unter.is_null() {
        continue;
      }
      let eintrag: *mut AnyObject = msg_send![unter, itemWithTitle: ns_titel];
      if !eintrag.is_null() {
        let kuerzel: *mut AnyObject = msg_send![eintrag, keyEquivalent];
        let maske: usize = msg_send![eintrag, keyEquivalentModifierMask];
        let text: *const c_char = msg_send![kuerzel, UTF8String];
        return Some(format!("{:?} maske={maske:#x}", CStr::from_ptr(text).to_string_lossy()));
      }
    }
    None
  }
}

/// Welche Ansicht bekäme einen Mausklick an dieser Stelle des Fensters, und zieht ein Klick dort das Fenster?
/// `x` von links, `von_oben` von der Oberkante, in Punkten. Für den Prüfstand: zeigt ohne Maus und Bildschirm,
/// wie hoch der ziehbare Streifen der Titelleiste ist und ab wo Klicks die Web-App erreichen.
#[cfg(debug_assertions)]
pub fn treffer(fenster: &WebviewWindow, x: f64, von_oben: f64) {
  #[repr(C)]
  #[derive(Clone, Copy)]
  struct Punkt(f64, f64);
  #[repr(C)]
  #[derive(Clone, Copy)]
  struct Rechteck(Punkt, Punkt);
  unsafe impl objc2::Encode for Punkt {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
  }
  unsafe impl objc2::Encode for Rechteck {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
      "CGRect",
      &[Punkt::ENCODING, objc2::Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING])],
    );
  }
  mit_ns_window(fenster, move |ns_window| unsafe {
    let inhalt: *mut AnyObject = msg_send![ns_window, contentView];
    let rahmen: *mut AnyObject = msg_send![inhalt, superview];
    if rahmen.is_null() {
      return;
    }
    let flaeche: Rechteck = msg_send![rahmen, frame];
    // Je nach macOS-Version zählt der Fensterrahmen von oben oder von unten.
    let gespiegelt: bool = msg_send![rahmen, isFlipped];
    let y = if gespiegelt { von_oben } else { flaeche.1 .1 - von_oben };
    let ansicht: *mut AnyObject = msg_send![rahmen, hitTest: Punkt(x, y)];
    if ansicht.is_null() {
      println!("[Prüfstand] treffer x={x} von_oben={von_oben}: keine Ansicht");
      return;
    }
    let name: *mut AnyObject = msg_send![ansicht, className];
    let zeichen: *const c_char = msg_send![name, UTF8String];
    let zieht: bool = msg_send![ansicht, mouseDownCanMoveWindow];
    println!("[Prüfstand] treffer x={x} von_oben={von_oben}: {} zieht_fenster={zieht}", CStr::from_ptr(zeichen).to_string_lossy());
  });
}
