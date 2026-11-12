// Anmeldung über Google/Apple im Browser des Nutzers statt im App-Fenster.
//
// Warum: Im eigenen Browser ist der Nutzer meist schon bei Google angemeldet, Passkeys, Sicherheitsschlüssel und
// Firmen-Logins funktionieren dort, und Google lehnt Passwort-Eingaben in eingebetteten Fenstern teils ab.
//
// Ablauf (die Web-App bleibt unverändert, sie ruft wie im Browser `signInWithOAuth`):
//   1. Das Hauptfenster will zum Anmelde-Server (`…/auth/v1/authorize?provider=…&redirect_to=…/auth/oauth`).
//      Der Navigations-Wächter (haupt.rs) fängt das ab und ruft `starte`.
//   2. Die Shell lauscht auf 127.0.0.1 (freier Port), tauscht `redirect_to` gegen diese Adresse und öffnet den Link im Browser.
//      Der Anmelde-Server erlaubt Rückkehr auf die eigene Maschine von sich aus (dafür ist sie bei Desktop-Apps gedacht).
//   3. Nach der Anmeldung landet der Browser auf der lokalen Seite (js/anmelde-seite.html). Sie reicht weiter, was der
//      Server mitgegeben hat (Sitzung im Fragment bzw. `error=` beim Abbruch), und zeigt „Du bist angemeldet".
//   4. Die Shell lädt im Hauptfenster die Rückkehr-Route der Web-App mit genau diesen Angaben und holt die App nach vorn.
//
// Schutz: Die Rückkehr-Adresse trägt ein Zufalls-Geheimnis im Pfad; nur wer es kennt, kann etwas übergeben. Sonst könnte
// eine fremde Webseite der App eine fremde Sitzung unterschieben. Angenommen wird genau eine Übergabe.
//
// Geht der Start schief (kein Port, kein Browser), bleibt die Anmeldung im Fenster — der Wächter lässt sie dann durch.

use std::{
  io::{ErrorKind, Read, Write},
  net::{Ipv4Addr, TcpListener, TcpStream},
  sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
  },
  thread,
  time::{Duration, Instant},
};

use tauri::{AppHandle, Url};
use tauri_plugin_opener::OpenerExt;

use crate::haupt;

const LOG: &str = "[Anmeldung]";
const SEITE: &str = include_str!("../js/anmelde-seite.html");
/// So lange wartet die Shell auf die Rückkehr aus dem Browser.
const FRIST: Duration = Duration::from_secs(10 * 60);
/// Nach der Übergabe antwortet die Shell noch kurz — ein Neuladen des Tabs zeigt dann einen Hinweis statt eines Browser-Fehlers.
const NACHLAUF: Duration = Duration::from_secs(60);
/// Zweiter Aufruf für denselben Klick (WebKit fragt gelegentlich doppelt): kein zweiter Tab.
const DOPPELKLICK: Duration = Duration::from_millis(1500);
const GROESSTE_ANFRAGE: usize = 64 * 1024;

/// Zählt die Anmelde-Versuche; ein neuer Versuch beendet den Lauscher des vorigen.
static LAUF: AtomicU64 = AtomicU64::new(0);
static ZULETZT: Mutex<Option<(Instant, String)>> = Mutex::new(None);

/// Ist das der Schritt, mit dem die Web-App die Anmeldung beim Anbieter beginnt?
pub fn ist_start(url: &Url) -> bool {
  url.scheme() == "https"
    && url.host_str() == Some(haupt::AUTH_SERVER)
    && url.path() == "/auth/v1/authorize"
    && url.query_pairs().any(|(name, _)| name == "provider")
}

/// Verlegt die Anmeldung in den Browser. `false` heißt: hat nicht geklappt, im Fenster weitermachen.
pub fn starte(app: &AppHandle, start: &Url) -> bool {
  {
    let mut zuletzt = ZULETZT.lock().unwrap();
    if zuletzt.as_ref().is_some_and(|(wann, adresse)| wann.elapsed() < DOPPELKLICK && adresse == start.as_str()) {
      return true;
    }
    *zuletzt = Some((Instant::now(), start.as_str().to_owned()));
  }

  // Wohin die Web-App zurückkehren wollte — nur ihre eigene Herkunft zählt, sonst die übliche Rückkehr-Route.
  let ziel = start
    .query_pairs()
    .find(|(name, _)| name == "redirect_to")
    .and_then(|(_, wert)| Url::parse(&wert).ok())
    .filter(haupt::ist_app_herkunft)
    .or_else(|| haupt::app_url().join("/auth/oauth").ok());
  let Some(ziel) = ziel else { return false };

  let Ok(lauscher) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else {
    eprintln!("{LOG} kein freier Port — Anmeldung bleibt im Fenster");
    return false;
  };
  let Some(port) = lauscher.local_addr().ok().map(|adresse| adresse.port()) else { return false };
  if lauscher.set_nonblocking(true).is_err() {
    return false;
  }
  let Some(geheim) = zufall() else { return false };

  let mut im_browser = start.clone();
  let uebrige: Vec<(String, String)> = start.query_pairs().filter(|(name, _)| name != "redirect_to").map(|(n, w)| (n.into_owned(), w.into_owned())).collect();
  im_browser.query_pairs_mut().clear().extend_pairs(uebrige).append_pair("redirect_to", &format!("http://127.0.0.1:{port}/{geheim}"));

  if let Err(fehler) = app.opener().open_url(im_browser.as_str(), None::<&str>) {
    eprintln!("{LOG} Browser ließ sich nicht öffnen ({fehler}) — Anmeldung bleibt im Fenster");
    return false;
  }

  // Nur Dev: die Rückkehr-Adresse für den Prüfstand (sie enthält das Geheimnis, gehört also in kein Release-Log).
  if crate::IS_DEV {
    println!("{LOG} im Browser geöffnet, Rückkehr auf http://127.0.0.1:{port}/{geheim}");
  }
  let lauf = LAUF.fetch_add(1, Ordering::SeqCst) + 1;
  let app = app.clone();
  thread::spawn(move || lausche(&app, &lauscher, port, &geheim, &ziel, lauf));
  true
}

/// 128 Bit Zufall aus dem Betriebssystem, als Hex.
fn zufall() -> Option<String> {
  let mut bytes = [0u8; 16];
  std::fs::File::open("/dev/urandom").and_then(|mut quelle| quelle.read_exact(&mut bytes)).ok()?;
  Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn lausche(app: &AppHandle, lauscher: &TcpListener, port: u16, geheim: &str, ziel: &Url, lauf: u64) {
  let uebergeben = Arc::new(AtomicBool::new(false));
  let mut ende = Instant::now() + FRIST;
  let mut im_nachlauf = false;
  while LAUF.load(Ordering::SeqCst) == lauf && Instant::now() < ende {
    if !im_nachlauf && uebergeben.load(Ordering::SeqCst) {
      im_nachlauf = true;
      ende = Instant::now() + NACHLAUF;
    }
    match lauscher.accept() {
      Ok((strom, _)) => {
        // Jede Verbindung für sich: Browser öffnen gern Verbindungen auf Vorrat, die nie etwas schicken.
        let (app, geheim, ziel, uebergeben) = (app.clone(), geheim.to_owned(), ziel.clone(), uebergeben.clone());
        thread::spawn(move || {
          let _ = bediene(strom, &app, port, &geheim, &ziel, &uebergeben);
        });
      }
      Err(fehler) if fehler.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(40)),
      Err(_) => break,
    }
  }
}

struct Anfrage {
  methode: String,
  pfad: String,
  host: String,
  inhalt: String,
}

fn lies(strom: &mut TcpStream) -> Option<Anfrage> {
  let mut puffer = Vec::new();
  let mut stueck = [0u8; 4096];
  let kopf_ende = loop {
    if let Some(ende) = puffer.windows(4).position(|f| f == b"\r\n\r\n") {
      break ende;
    }
    let gelesen = strom.read(&mut stueck).ok().filter(|n| *n > 0)?;
    puffer.extend_from_slice(&stueck[..gelesen]);
    if puffer.len() > GROESSTE_ANFRAGE {
      return None;
    }
  };
  let kopf = String::from_utf8_lossy(&puffer[..kopf_ende]).into_owned();
  let mut zeilen = kopf.split("\r\n");
  let mut erste = zeilen.next()?.split(' ');
  let (methode, pfad) = (erste.next()?.to_owned(), erste.next()?.to_owned());
  let (mut host, mut laenge) = (String::new(), 0usize);
  for zeile in zeilen {
    let Some((name, wert)) = zeile.split_once(':') else { continue };
    match name.trim().to_ascii_lowercase().as_str() {
      "host" => host = wert.trim().to_owned(),
      "content-length" => laenge = wert.trim().parse().ok()?,
      _ => {}
    }
  }
  if laenge > GROESSTE_ANFRAGE {
    return None;
  }
  let mut inhalt = puffer[kopf_ende + 4..].to_vec();
  while inhalt.len() < laenge {
    let gelesen = strom.read(&mut stueck).ok().filter(|n| *n > 0)?;
    inhalt.extend_from_slice(&stueck[..gelesen]);
  }
  inhalt.truncate(laenge);
  Some(Anfrage { methode, pfad, host, inhalt: String::from_utf8(inhalt).ok()? })
}

fn antworte(strom: &mut TcpStream, status: &str, inhalt: &str) -> std::io::Result<()> {
  write!(
    strom,
    "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{inhalt}",
    inhalt.len()
  )?;
  strom.flush()
}

fn bediene(mut strom: TcpStream, app: &AppHandle, port: u16, geheim: &str, ziel: &Url, uebergeben: &AtomicBool) -> std::io::Result<()> {
  strom.set_nonblocking(false)?;
  strom.set_read_timeout(Some(Duration::from_secs(5)))?;
  strom.set_write_timeout(Some(Duration::from_secs(5)))?;
  let Some(anfrage) = lies(&mut strom) else { return Ok(()) };
  // Nur Aufrufe, die wirklich an diese Adresse gerichtet sind (kein fremder Name, der auf 127.0.0.1 zeigt).
  if anfrage.host != format!("127.0.0.1:{port}") {
    return antworte(&mut strom, "403 Forbidden", "");
  }
  let pfad = anfrage.pfad.split(['?', '#']).next().unwrap_or_default();
  let seite = |zustand: &str| SEITE.replace("__GEHEIM__", geheim).replace("__ZUSTAND__", zustand);
  match (anfrage.methode.as_str(), pfad == format!("/{geheim}")) {
    // Rückkehr aus dem Browser: die Seite liest ihre Adresse aus und schickt sie per POST hierher.
    ("GET", true) => antworte(&mut strom, "200 OK", &seite(if uebergeben.load(Ordering::SeqCst) { "erledigt" } else { "offen" })),
    ("POST", true) => {
      if uebergeben.swap(true, Ordering::SeqCst) {
        return antworte(&mut strom, "409 Conflict", "");
      }
      antworte(&mut strom, "204 No Content", "")?;
      uebergib(app, ziel, &anfrage.inhalt);
      Ok(())
    }
    // Neu geladener Tab (die Seite hat ihre Adresse auf „/" gekürzt).
    ("GET", false) if pfad == "/" => antworte(&mut strom, "200 OK", &seite("erledigt")),
    _ => antworte(&mut strom, "404 Not Found", ""),
  }
}

/// Setzt die Angaben des Anmelde-Servers (`?…#…`) an die Rückkehr-Route der Web-App.
fn rueckkehr_adresse(ziel: &Url, angaben: &str) -> Url {
  let (suche, fragment) = angaben.split_once('#').unwrap_or((angaben, ""));
  let mut adresse = ziel.clone();
  adresse.set_query(suche.strip_prefix('?').filter(|s| !s.is_empty()));
  adresse.set_fragment(Some(fragment).filter(|f| !f.is_empty()));
  adresse
}

fn uebergib(app: &AppHandle, ziel: &Url, angaben: &str) {
  let adresse = rueckkehr_adresse(ziel, angaben);
  // Erst nach vorn (erzeugt das Hauptfenster, falls es geschlossen wurde), dann die Rückkehr-Route laden.
  haupt::hole_app_nach_vorn(app);
  match haupt::fenster(app).map(|fenster| fenster.navigate(adresse)) {
    Some(Ok(())) => {}
    Some(Err(fehler)) => eprintln!("{LOG} Rückkehr ließ sich nicht laden: {fehler}"),
    None => eprintln!("{LOG} kein Hauptfenster für die Rückkehr"),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn erkennt_den_start() {
    let ja = Url::parse("https://api.prepwell.de/auth/v1/authorize?provider=google&redirect_to=https%3A%2F%2Fapp.prepwell.de%2Fauth%2Foauth").unwrap();
    assert!(ist_start(&ja));
    for nein in ["https://api.prepwell.de/auth/v1/callback?code=x", "https://api.prepwell.de/auth/v1/authorize", "https://evil.example/auth/v1/authorize?provider=google", "http://api.prepwell.de/auth/v1/authorize?provider=google"] {
      assert!(!ist_start(&Url::parse(nein).unwrap()), "{nein}");
    }
  }

  #[test]
  fn rueckkehr_traegt_sitzung_und_abbruch() {
    let ziel = Url::parse("https://app.prepwell.de/auth/oauth").unwrap();
    assert_eq!(rueckkehr_adresse(&ziel, "#access_token=a.b.c&refresh_token=r&token_type=bearer").as_str(), "https://app.prepwell.de/auth/oauth#access_token=a.b.c&refresh_token=r&token_type=bearer");
    assert_eq!(rueckkehr_adresse(&ziel, "?error=access_denied&error_description=x#error=access_denied").as_str(), "https://app.prepwell.de/auth/oauth?error=access_denied&error_description=x#error=access_denied");
    assert_eq!(rueckkehr_adresse(&ziel, "").as_str(), "https://app.prepwell.de/auth/oauth");
  }
}
