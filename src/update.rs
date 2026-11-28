//! Aviso de versao nova. Arquivo identico em todos os apps (como o `header.rs`).
//!
//! Le o `version.txt` da branch main no GitHub com o `curl` do sistema, para nao embutir
//! TLS no app, e diz se ha uma versao mais nova que a deste binario. `draw_dialog` desenha o aviso por cima do quadro pronto.

use std::process::{Command, Stdio};

use crate::gfx::{Color, Face, Fonts, Pixmap};

/// Versao deste binario (a do `Cargo.toml`, igual ao `version.txt` da raiz).
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// `version.txt` publicado na branch main (o repositorio tem o nome do pacote).
pub const VERSION_URL: &str = concat!(
    "https://raw.githubusercontent.com/tbipsoft/",
    env!("CARGO_PKG_NAME"),
    "/main/version.txt"
);

/// Onde o usuario baixa a versao nova.
pub const RELEASES_URL: &str = concat!("github.com/tbipsoft/", env!("CARGO_PKG_NAME"), "/releases");

// ---------------------------------------------------------------------------
// Versao
// ---------------------------------------------------------------------------

/// Versao no formato `MAIOR.MENOR.PATCH[-pre]` (semver sem metadados de build).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

fn parse(s: &str) -> Option<Version> {
    let s = s.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    let s = s.split('+').next()?; // ignora metadados de build
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    let mut nums = core.split('.');
    let mut out = [0u64; 3];
    for n in &mut out {
        let part = nums.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *n = part.parse().ok()?;
    }
    if nums.next().is_some() {
        return None;
    }
    let pre = match pre {
        None => Vec::new(),
        Some(p) => {
            let ids: Vec<String> = p.split('.').map(str::to_string).collect();
            let ok = |id: &String| {
                !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            };
            if !ids.iter().all(ok) {
                return None;
            }
            ids
        }
    };
    Some(Version { core: out, pre })
}

/// Precedencia do semver: o nucleo decide; com nucleo igual, uma versao sem `-pre` vence;
/// entre pre-releases, compara identificador a identificador (numeros como numeros e
/// antes de texto).
fn cmp(a: &Version, b: &Version) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match a.core.cmp(&b.core) {
        Equal => {}
        o => return o,
    }
    match (a.pre.is_empty(), b.pre.is_empty()) {
        (true, true) => return Equal,
        (true, false) => return Greater,
        (false, true) => return Less,
        _ => {}
    }
    for (x, y) in a.pre.iter().zip(&b.pre) {
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(n), Ok(m)) => n.cmp(&m),
            (Ok(_), Err(_)) => Less,
            (Err(_), Ok(_)) => Greater,
            _ => x.cmp(y),
        };
        if o != Equal {
            return o;
        }
    }
    a.pre.len().cmp(&b.pre.len())
}

/// `Some(remota)` (ja limpa) se `remote` for valida e mais nova que `current`.
pub fn newer(remote: &str, current: &str) -> Option<String> {
    let r = parse(remote)?;
    let c = parse(current)?;
    (cmp(&r, &c) == std::cmp::Ordering::Greater).then(|| remote.trim().to_string())
}

/// Busca a versao publicada e devolve `Some(versao)` se for mais nova que esta.
/// Qualquer falha (sem rede, sem `curl`, resposta estranha) vira `None`: sem aviso.
/// Bloqueia ate ~6 s; chame numa thread.
pub fn check() -> Option<String> {
    let out = Command::new("curl")
        .args(["-fsL", "--max-time", "6", VERSION_URL])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.len() > 64 {
        return None;
    }
    newer(std::str::from_utf8(&out.stdout).ok()?, CURRENT)
}

/// Inicia a verificacao numa thread. `preview` (opcao `--update-preview`) pula a rede e
/// mostra o aviso com essa versao. Leia o resultado com `try_recv` a cada quadro.
pub fn spawn(enabled: bool, preview: Option<String>) -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(v) = preview {
        let _ = tx.send(v);
    } else if enabled {
        std::thread::spawn(move || {
            if let Some(v) = check() {
                eprintln!("versao nova disponivel: {v} (esta e {CURRENT})");
                let _ = tx.send(v);
            }
        });
    }
    rx
}

// ---------------------------------------------------------------------------
// Aviso na tela
// ---------------------------------------------------------------------------

const BG: Color = Color::hex(0x0B0D12);
const CARD: Color = Color::hex(0x12161F);
const LINE: Color = Color::hex(0x1D2332);
const TEXT: Color = Color::hex(0xE8ECF4);
const MUTED: Color = Color::hex(0x8E98AD);
const KEYCAP_BG: Color = Color::hex(0x1B2130);
const KEYCAP_BORDER: Color = Color::hex(0x2A3144);

const BOX_W: f32 = 520.0;
const BOX_H: f32 = 236.0;

/// Escala e retangulo do aviso (em pixels) para um quadro `w` x `h`. A largura de
/// referencia segue as telas dos apps: 960 em 16:9, 640 em 4:3 e 560 em 3:2.
fn layout(w: f32, h: f32) -> (f32, f32, f32, f32, f32) {
    let aspect = w / h;
    let ref_w = if aspect >= 1.6 {
        960.0
    } else if aspect >= 1.45 {
        560.0
    } else {
        640.0
    };
    let k = (w / ref_w).min(h / (BOX_H + 24.0));
    let bw = (ref_w - 24.0).min(BOX_W);
    let (pw, ph) = (bw * k, BOX_H * k);
    (k, (w - pw) / 2.0, (h - ph) / 2.0, pw, ph)
}

/// Desenha o aviso "ha uma versao nova" por cima de `pm`, escurecendo o resto da tela.
pub fn draw_dialog(pm: &mut Pixmap, fonts: &Fonts, new: &str, accent: Color) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    if w < 1.0 || h < 1.0 {
        return;
    }
    pm.fill_rect(0.0, 0.0, w, h, BG.alpha(0.78));
    let (k, bx, by, bw, bh) = layout(w, h);
    pm.fill_rrect(bx, by, bw, bh, 18.0 * k, CARD);
    pm.stroke_rrect(bx, by, bw, bh, 18.0 * k, k.max(1.0), LINE);

    // texto com o centro vertical em `cy` (coordenadas do aviso), cortado na largura
    let (x, max_w) = (bx + 24.0 * k, bw - 48.0 * k);
    let line =
        |pm: &mut Pixmap, face: Face, size: f32, sp: f32, c: Color, dx: f32, cy: f32, s: &str| {
            let s = fonts.fit(face, size * k, sp * k, s, max_w - dx * k);
            let (a, d) = fonts.metrics(face);
            let base = by + (cy + (a + d) * size / 2.0) * k;
            pm.text(fonts, face, size * k, sp * k, c, x + dx * k, base, &s)
        };

    pm.fill_circle(x + 3.5 * k, by + 30.0 * k, 3.5 * k, accent);
    line(
        pm,
        Face::Sans800,
        12.0,
        1.92,
        MUTED,
        14.0,
        30.0,
        "UPDATE AVAILABLE",
    );
    line(
        pm,
        Face::Mono700,
        26.0,
        0.0,
        accent,
        0.0,
        70.0,
        &format!("Version {new}"),
    );
    let have = format!("You have version {CURRENT}.");
    line(pm, Face::Sans600, 13.0, 0.0, MUTED, 0.0, 100.0, &have);
    line(
        pm,
        Face::Sans600,
        13.0,
        0.0,
        TEXT,
        0.0,
        128.0,
        "To update, download the new version from",
    );
    line(
        pm,
        Face::Mono700,
        13.0,
        0.0,
        accent,
        0.0,
        150.0,
        RELEASES_URL,
    );
    line(
        pm,
        Face::Sans600,
        13.0,
        0.0,
        TEXT,
        0.0,
        172.0,
        "and replace the files in roms/tools/Tbip.",
    );

    // tecla "A" + "OK"
    let cy = BOX_H - 26.0;
    let kw = (fonts.width(Face::Mono700, 11.0 * k, 0.0, "A") / k + 12.0).max(24.0);
    let (kx, ky) = (x, by + (cy - 12.0) * k);
    pm.fill_rrect(kx, ky, kw * k, 24.0 * k, 7.0 * k, KEYCAP_BG);
    pm.stroke_rrect(kx, ky, kw * k, 24.0 * k, 7.0 * k, k.max(1.0), KEYCAP_BORDER);
    let tw = fonts.width(Face::Mono700, 11.0 * k, 0.0, "A") / k;
    line(pm, Face::Mono700, 11.0, 0.0, TEXT, (kw - tw) / 2.0, cy, "A");
    line(pm, Face::Sans600, 13.0, 0.0, MUTED, kw + 8.0, cy, "OK");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versao_mais_nova() {
        assert_eq!(newer("1.0.1", "1.0.0").as_deref(), Some("1.0.1"));
        assert_eq!(newer("1.1.0\n", "1.0.9").as_deref(), Some("1.1.0"));
        assert_eq!(newer("2.0.0", "1.99.99").as_deref(), Some("2.0.0"));
        assert_eq!(newer("1.10.0", "1.9.0").as_deref(), Some("1.10.0"));
        assert_eq!(newer("v1.0.1", "1.0.0").as_deref(), Some("v1.0.1"));
    }

    #[test]
    fn versao_igual_ou_mais_velha_nao_avisa() {
        assert_eq!(newer("1.0.0", "1.0.0"), None);
        assert_eq!(newer("1.0.0-alpha", "1.0.0-alpha"), None);
        assert_eq!(newer("0.9.0", "1.0.0-alpha"), None);
        assert_eq!(newer("1.0.0-alpha", "1.0.0"), None);
    }

    #[test]
    fn pre_release() {
        // a versao final vence a pre-release do mesmo numero
        assert!(newer("1.0.0", "1.0.0-alpha").is_some());
        assert!(newer("1.0.0-beta", "1.0.0-alpha").is_some());
        assert!(newer("1.0.0-alpha.2", "1.0.0-alpha.1").is_some());
        assert!(newer("1.0.0-alpha.10", "1.0.0-alpha.9").is_some());
        assert!(newer("1.0.0-alpha.1", "1.0.0-alpha").is_some());
        assert!(newer("1.0.0-alpha.beta", "1.0.0-alpha.1").is_some());
    }

    #[test]
    fn resposta_invalida_nao_avisa() {
        for r in [
            "",
            "404: Not Found",
            "<html>",
            "1.0",
            "1.0.0.0",
            "1..0",
            "1.0.0-",
            "a.b.c",
        ] {
            assert_eq!(newer(r, "1.0.0-alpha"), None, "{r:?}");
        }
    }

    #[test]
    fn versao_do_binario_e_urls() {
        assert!(parse(CURRENT).is_some(), "{CURRENT}");
        let name = env!("CARGO_PKG_NAME");
        assert!(VERSION_URL.ends_with(&format!("/tbipsoft/{name}/main/version.txt")));
        assert_eq!(RELEASES_URL, format!("github.com/tbipsoft/{name}/releases"));
    }

    #[test]
    fn preview_chega_sem_rede() {
        let rx = spawn(false, Some("9.9.9".into()));
        assert_eq!(rx.recv().unwrap(), "9.9.9");
        assert!(spawn(false, None).recv().is_err());
    }

    /// `Pixmap::get` devolve (r, g, b, a).
    fn near(a: [u8; 4], b: Color, tol: i32) -> bool {
        (a[0] as i32 - b.r as i32).abs() <= tol
            && (a[1] as i32 - b.g as i32).abs() <= tol
            && (a[2] as i32 - b.b as i32).abs() <= tol
    }

    #[test]
    fn aviso_cabe_e_escurece_o_fundo() {
        let fonts = Fonts::new();
        for (w, h) in [
            (1280, 720),
            (960, 540),
            (640, 480),
            (480, 320),
            (720, 720),
            (1024, 768),
        ] {
            let (wf, hf) = (w as f32, h as f32);
            let (_, x, y, bw, bh) = layout(wf, hf);
            assert!(x >= 0.0 && x + bw <= wf, "{w}x{h}");
            assert!(y >= 0.0 && y + bh <= hf, "{w}x{h}");
            assert!(bw >= wf * 0.5, "{w}x{h}: aviso pequeno demais");

            let mut pm = Pixmap::new(w, h);
            pm.clear(CARD);
            draw_dialog(&mut pm, &fonts, "1.2.0", Color::hex(0xFFB938));
            // canto da tela: escurecido; canto do aviso sem texto: cor do cartao
            assert!(
                near(pm.get(1, 1).unwrap(), CARD.lerp(BG, 0.78), 4),
                "{w}x{h}"
            );
            let inside = pm
                .get((x + bw - 30.0) as usize, (y + 30.0) as usize)
                .unwrap();
            assert!(near(inside, CARD, 3), "{w}x{h} {inside:?}");
        }
        // tamanhos degenerados nao quebram
        draw_dialog(
            &mut Pixmap::new(1, 1),
            &fonts,
            "1.2.0",
            Color::hex(0xFFB938),
        );
    }

    #[test]
    fn captura_do_aviso() {
        if let Ok(p) = std::env::var("UPDATE_DIALOG_DUMP") {
            for (w, h) in [(1280, 720), (640, 480)] {
                let mut pm = Pixmap::new(w, h);
                pm.clear(CARD);
                draw_dialog(&mut pm, &Fonts::new(), "1.2.0", Color::hex(0xFFB938));
                pm.write_ppm(std::path::Path::new(&format!("{p}-{w}x{h}.ppm")))
                    .unwrap();
            }
        }
    }
}
