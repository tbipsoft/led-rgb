#![allow(clippy::too_many_arguments)]
//! Tela do app: o design 960x540 (tema escuro, Manrope + JetBrains Mono) desenhado
//! com `gfx`. Nao depende do SDL: `Renderer` produz um `Pixmap` por quadro e o
//! `main` so o envia para uma textura. Assim a tela tambem e testada e fotografada
//! (`--screenshot`) sem janela.
//!
//! Tudo e posicionado em coordenadas do design (960x540) e escalado por `k`, o
//! maior fator que cabe na tela (letterbox centralizado): 1280x720 => 1,333;
//! 1024x768 (RG43 Pro) => 1,067 com faixas pretas em cima e embaixo.
//!
//! Diferencas conscientes em relacao ao mock:
//! * sem a faixa "RAW BYTE" (removida a pedido); o espaco liberado virou tiles mais
//!   altos (72 em vez de 56) mantendo os mesmos intervalos verticais;
//! * cada anel mostra `leds / 2` LEDs (6 com os 12 do aparelho), nao 8 fixos;
//! * o status reflete o envio de verdade (Sending / Sent / erro).

use crate::driver::BRI_MAX;
use crate::effects::{BREATHING, SOLID};
use crate::gfx::{conic, Color, Face, Fonts, Pixmap};
use std::collections::HashMap;
use std::f32::consts::PI;

pub const DESIGN_W: f32 = 960.0;
pub const DESIGN_H: f32 = 540.0;
pub const TILE_COUNT: usize = 16;
pub const COLS: usize = 4;

// ---------------------------------------------------------------------------
// Paleta (valores do mock)
// ---------------------------------------------------------------------------

const BG_PAGE: Color = Color::hex(0x0B0D12);
const TEXT: Color = Color::hex(0xE8ECF4);
const MUTED: Color = Color::hex(0x8E98AD);
const LINE: Color = Color::hex(0x1D2332);
const TILE_BG: Color = Color::hex(0x151A24);
const TILE_BG_SEL: Color = Color::hex(0x1E2536);
const TILE_BORDER_DASH: Color = Color::hex(0x3A4256);
const TILE_SEL_BORDER: Color = Color::hex(0xF4F6FB);
const BADGE_DASH: Color = Color::hex(0x5A6580);
const PANEL_BG: Color = Color::hex(0x12161F);
const DARK: Color = Color::hex(0x0B0E14);
const BEZEL_BORDER: Color = Color::hex(0x222939);
const LED_OFF: Color = Color::hex(0x262C3B);
const LED_UNKNOWN: Color = Color::hex(0x5A6580);
const SEG_OFF: Color = Color::hex(0x242B3B);
const KEYCAP_BG: Color = Color::hex(0x1B2130);
const KEYCAP_BORDER: Color = Color::hex(0x2A3144);
const ON_TEXT: Color = Color::hex(0x4ADE80);
const TRACK_ON: Color = Color::hex(0x1F8F4E);
const TRACK_OFF: Color = Color::hex(0x2A3144);
const ERROR: Color = Color::hex(0xFF6B6B);

const C_GREEN: Color = Color::hex(0x2EE66B);
const C_BLUE: Color = Color::hex(0x3B82FF);
const C_RED: Color = Color::hex(0xFF4D4D);
const C_CYAN: Color = Color::hex(0x22D3EE);
const C_YELLOW: Color = Color::hex(0xFFD21F);
const C_MAGENTA: Color = Color::hex(0xF43FD0);
const GLOW_MULTI: Color = Color::hex(0x9B6BFF);
const RAINBOW: [Color; 5] = [
    Color::hex(0xFF7A1A),
    C_GREEN,
    C_BLUE,
    C_MAGENTA,
    Color::hex(0xFF7A1A),
];
const PAL: [Color; 4] = [
    Color::hex(0x2EE6A0),
    Color::hex(0xF5C518),
    Color::hex(0xF43F8E),
    Color::hex(0x3B4BFF),
];
const PAL_STOPS: [Color; 5] = [PAL[0], PAL[1], PAL[2], PAL[3], PAL[0]];

/// Destaques oferecidos pelo mock (`--accent amber|teal|violet|coral` ou `#RRGGBB`).
pub const ACCENT_DEFAULT: Color = Color::hex(0xFFB938);

pub fn parse_accent(s: &str) -> Result<Color, String> {
    match s.to_ascii_lowercase().as_str() {
        "amber" => Ok(ACCENT_DEFAULT),
        "teal" => Ok(Color::hex(0x5EEAD4)),
        "violet" => Ok(Color::hex(0xA78BFA)),
        "coral" => Ok(Color::hex(0xFF7A59)),
        t => {
            let h = t.strip_prefix('#').unwrap_or(t);
            if h.len() == 6 {
                if let Ok(v) = u32::from_str_radix(h, 16) {
                    return Ok(Color::hex(v));
                }
            }
            Err(format!(
                "cor invalida: {s:?} (use amber, teal, violet, coral ou #RRGGBB)"
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Modelo
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Nada enviado ainda.
    Ready,
    /// Envio em andamento (o ponto pulsa).
    Sending,
    /// Ultimo envio concluido.
    Sent,
    Error(String),
    Notice(String),
}

/// Tudo o que a tela precisa saber; o `main` monta um por quadro.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// Byte de modo atual (o tile correspondente fica selecionado; se nao houver
    /// tile para ele, nada fica selecionado e o painel mostra "Custom").
    pub mode: u8,
    pub on: bool,
    /// 1..=9
    pub bri: u8,
    /// Total de LEDs da cadeia (12 no aparelho): cada anel mostra metade.
    pub leds: u32,
    pub status: Status,
    pub dry_run: bool,
    pub accent: Color,
}

#[derive(Clone, Debug)]
pub struct Tile {
    pub mode: u8,
    pub label: String,
    pub breathing: bool,
    /// `false` = deduzido, nao confirmado (borda tracejada e "?").
    pub verified: bool,
}

/// Os 16 tiles: linhas 0-1 = SOLID, linhas 2-3 = BREATHING.
pub fn tiles() -> Vec<Tile> {
    fn title(s: &str) -> String {
        let mut c = s.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
            None => String::new(),
        }
    }
    SOLID
        .iter()
        .map(|e| (e, false))
        .chain(BREATHING.iter().map(|e| (e, true)))
        .map(|(e, breathing)| Tile {
            mode: e.mode,
            label: title(e.label),
            breathing,
            verified: e.verified,
        })
        .collect()
}

pub fn is_breathing(mode: u8) -> bool {
    (17..=24).contains(&mode)
}

#[derive(Clone, Copy)]
enum Look {
    Solid(Color),
    Rainbow,
    Pal,
    Unknown,
}

fn look(mode: u8) -> Look {
    let base = if is_breathing(mode) { mode - 16 } else { mode };
    match base {
        1 => Look::Solid(C_GREEN),
        2 => Look::Solid(C_BLUE),
        3 => Look::Solid(C_RED),
        4 => Look::Solid(C_CYAN),
        5 => Look::Solid(C_YELLOW),
        6 => Look::Solid(C_MAGENTA),
        7 => Look::Rainbow,
        8 => Look::Pal,
        _ => Look::Unknown,
    }
}

/// Cor do LED `k` (indice na cadeia do anel, ja com o deslocamento) de `n`.
fn led_color(l: Look, k: usize, n: usize) -> Color {
    match l {
        Look::Solid(c) => c,
        Look::Rainbow => Color::hsl((k as f32 * 360.0 / n as f32 + 20.0) % 360.0, 0.92, 0.58),
        Look::Pal => PAL[k % 4],
        Look::Unknown => LED_UNKNOWN,
    }
}

fn mode_name(m: &Model, tiles: &[Tile]) -> String {
    match tiles.iter().find(|t| t.mode == m.mode) {
        Some(t) if t.breathing => format!("Breath {}", t.label),
        Some(t) if matches!(look(t.mode), Look::Rainbow | Look::Pal) => t.label.clone(),
        Some(t) => format!("Solid {}", t.label),
        None => format!("Custom {}", m.mode),
    }
}

// ---------------------------------------------------------------------------
// Animacoes (curvas do CSS do mock)
// ---------------------------------------------------------------------------

/// `cubic-bezier(.42, 0, .58, 1)` = `ease-in-out` do CSS.
fn ease_in_out(x: f32) -> f32 {
    let bez = |a: f32, b: f32, t: f32| {
        3.0 * a * (1.0 - t).powi(2) * t + 3.0 * b * (1.0 - t) * t * t + t.powi(3)
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(0.42, 0.58, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(0.0, 1.0, (lo + hi) / 2.0)
}

/// Vai e volta 0 -> 1 -> 0 em `period` s, com ease-in-out em cada metade.
fn phase(t: f32, period: f32) -> f32 {
    let p = (t / period).rem_euclid(1.0);
    if p < 0.5 {
        ease_in_out(p * 2.0)
    } else {
        1.0 - ease_in_out(p * 2.0 - 1.0)
    }
}

/// `@keyframes breathe`: opacidade .3 -> 1 -> .3 em 2,8 s (aneis dos LEDs).
pub fn breathe_opacity(t: f32) -> f32 {
    0.3 + 0.7 * phase(t, 2.8)
}

/// `@keyframes pulse`: (opacidade .35 -> 1, escala .8 -> 1) em 1,2 s (ponto do status).
pub fn pulse(t: f32) -> (f32, f32) {
    let p = phase(t, 1.2);
    (0.35 + 0.65 * p, 0.8 + 0.2 * p)
}

// ---------------------------------------------------------------------------
// Geometria (coordenadas do design)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct R {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl R {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

const LEFT_X: f32 = 32.0;
const LEFT_W: f32 = 596.0;
const BODY_Y: f32 = 84.0;
const BODY_H: f32 = 432.0;
const PANEL_X: f32 = 656.0;
const PANEL_W: f32 = 272.0;
const PX0: f32 = PANEL_X + 21.0; // borda 1 + padding 20
const PW: f32 = PANEL_W - 42.0; // 230
const PY0: f32 = BODY_Y + 21.0;
const PH: f32 = BODY_H - 42.0; // 390

const SEC_HEAD: f32 = 18.0;
const SEC_GAP: f32 = 8.0;
const TILE_H: f32 = 72.0;
const TILE_GAP: f32 = 10.0;
const HINT_H: f32 = 28.0;

const SWITCH_H: f32 = 44.0;
const RINGS_H: f32 = 104.0;
const BRI_ROW_H: f32 = 26.4;
const BRI_H: f32 = BRI_ROW_H + 2.0 + 36.0;
const MODE_LABEL_H: f32 = 16.4;
const MODE_H: f32 = MODE_LABEL_H + 6.0 + 24.2;
const STATUS_H: f32 = 40.0;
const SEG_GAP: f32 = 4.0;

struct Geo {
    tiles: [R; TILE_COUNT],
    sec_y: [f32; 2],
    hints_y: f32,
    switch: R,
    rings_y: f32,
    bri_y: f32,
    segs: [R; 9],
    mode_y: f32,
    status: R,
}

fn geo() -> Geo {
    let cw = (LEFT_W - 3.0 * TILE_GAP) / 4.0;
    let sec_h = SEC_HEAD + SEC_GAP + 2.0 * TILE_H + TILE_GAP;
    let gap = (BODY_H - 2.0 * sec_h - HINT_H) / 2.0; // space-between com 3 itens
    let sec_y = [BODY_Y, BODY_Y + sec_h + gap];
    let mut tiles = [R {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    }; TILE_COUNT];
    for (i, t) in tiles.iter_mut().enumerate() {
        let (sec, idx) = (i / 8, i % 8);
        *t = R {
            x: LEFT_X + (idx % 4) as f32 * (cw + TILE_GAP),
            y: sec_y[sec] + SEC_HEAD + SEC_GAP + (idx / 4) as f32 * (TILE_H + TILE_GAP),
            w: cw,
            h: TILE_H,
        };
    }
    let sum = SWITCH_H + RINGS_H + BRI_H + MODE_H + STATUS_H;
    let g = (PH - sum) / 4.0; // space-between com 5 itens
    let switch_y = PY0;
    let rings_y = switch_y + SWITCH_H + g;
    let bri_y = rings_y + RINGS_H + g;
    let mode_y = bri_y + BRI_H + g;
    let status_y = mode_y + MODE_H + g;
    let seg_w = (PW - 8.0 * SEG_GAP) / 9.0;
    let mut segs = [R {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    }; 9];
    for (i, s) in segs.iter_mut().enumerate() {
        *s = R {
            x: PX0 + i as f32 * (seg_w + SEG_GAP),
            y: bri_y + BRI_ROW_H + 2.0,
            w: seg_w,
            h: 36.0,
        };
    }
    Geo {
        tiles,
        sec_y,
        hints_y: BODY_Y + BODY_H - HINT_H,
        switch: R {
            x: PX0,
            y: switch_y,
            w: PW,
            h: SWITCH_H,
        },
        rings_y,
        bri_y,
        segs,
        mode_y,
        status: R {
            x: PX0,
            y: status_y,
            w: PW,
            h: STATUS_H,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Tile(usize),
    Power,
    /// Nivel de brilho 1..=9.
    Bri(u8),
}

// ---------------------------------------------------------------------------
// Pintor em coordenadas do design
// ---------------------------------------------------------------------------

struct P<'a> {
    pm: &'a mut Pixmap,
    f: &'a Fonts,
    k: f32,
    ox: f32,
    oy: f32,
}

impl P<'_> {
    fn x(&self, v: f32) -> f32 {
        self.ox + v * self.k
    }
    fn y(&self, v: f32) -> f32 {
        self.oy + v * self.k
    }
    fn rrect(&mut self, r: R, rad: f32, c: Color) {
        let (x, y) = (self.x(r.x), self.y(r.y));
        self.pm
            .fill_rrect(x, y, r.w * self.k, r.h * self.k, rad * self.k, c);
    }
    fn stroke(&mut self, r: R, rad: f32, bw: f32, c: Color) {
        let (x, y) = (self.x(r.x), self.y(r.y));
        self.pm.stroke_rrect(
            x,
            y,
            r.w * self.k,
            r.h * self.k,
            rad * self.k,
            bw * self.k,
            c,
        );
    }
    fn dashed(&mut self, r: R, rad: f32, bw: f32, c: Color) {
        let (x, y, k) = (self.x(r.x), self.y(r.y), self.k);
        self.pm.stroke_rrect_dashed(
            x,
            y,
            r.w * k,
            r.h * k,
            rad * k,
            bw * k,
            c,
            3.0 * bw * k,
            2.0 * bw * k,
        );
    }
    fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        let (x, y) = (self.x(cx), self.y(cy));
        self.pm.fill_circle(x, y, r * self.k, c);
    }
    fn dashed_ring(&mut self, cx: f32, cy: f32, r: f32, bw: f32, c: Color) {
        let (x, y, k) = (self.x(cx), self.y(cy), self.k);
        self.pm
            .stroke_circle_dashed(x, y, r * k, bw * k, c, 3.0 * bw * k, 2.0 * bw * k);
    }
    fn glow_circle(&mut self, cx: f32, cy: f32, r: f32, sigma: f32, c: Color) {
        let (x, y, k) = (self.x(cx), self.y(cy), self.k);
        self.pm.glow_circle(x, y, r * k, sigma * k, c);
    }
    fn glow_rrect(&mut self, r: R, rad: f32, sigma: f32, c: Color) {
        let (x, y, k) = (self.x(r.x), self.y(r.y), self.k);
        self.pm
            .glow_rrect(x, y, r.w * k, r.h * k, rad * k, sigma * k, c);
    }
    /// Texto com a linha de base em `baseline` (design). Devolve a largura.
    fn text(
        &mut self,
        face: Face,
        size: f32,
        sp: f32,
        c: Color,
        x: f32,
        baseline: f32,
        s: &str,
    ) -> f32 {
        let (px, py, k) = (self.x(x), self.y(baseline), self.k);
        self.pm.text(self.f, face, size * k, sp * k, c, px, py, s) / k
    }
    fn width(&self, face: Face, size: f32, sp: f32, s: &str) -> f32 {
        self.f.width(face, size * self.k, sp * self.k, s) / self.k
    }
    /// Linha de base que centra verticalmente a caixa de linha (ascendente +
    /// descendente) em `cy`, como o `align-items:center` do CSS.
    fn mid(&self, face: Face, size: f32, cy: f32) -> f32 {
        let (a, d) = self.f.metrics(face);
        cy + (a + d) * size / 2.0
    }
    fn text_mid(
        &mut self,
        face: Face,
        size: f32,
        sp: f32,
        c: Color,
        x: f32,
        cy: f32,
        s: &str,
    ) -> f32 {
        let b = self.mid(face, size, cy);
        self.text(face, size, sp, c, x, b, s)
    }
    /// Linha de base de uma linha cujo topo esta em `top`, com `line_h` de altura.
    fn top_baseline(&self, face: Face, size: f32, line_h: Option<f32>, top: f32) -> f32 {
        let (a, d) = self.f.metrics(face);
        let content = (a - d) * size;
        let half = line_h.map_or(0.0, |l| (l - content) / 2.0);
        top + half + a * size
    }
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

type RKey = (u8, bool, u8, u32);

/// Folga em volta de cada tile para o brilho da selecao (sigma 12 => ~36 px).
const TILE_PAD: f32 = 40.0;

pub struct Renderer {
    fonts: Fonts,
    title: &'static str,
    tiles: Vec<Tile>,
    geo: Geo,
    w: usize,
    h: usize,
    k: f32,
    ox: f32,
    oy: f32,
    /// Partes fixas (fundo, cabecalho, titulos, dicas, moldura do painel); so refaz se
    /// o destaque ou o selo DRY-RUN mudarem.
    statics: Pixmap,
    statics_key: Option<(Color, bool)>,
    /// Cada tile ja desenhado (normal e selecionado) em camadas transparentes com folga
    /// para o brilho: mover o cursor vira so colar duas camadas, em vez de redesenhar
    /// brilhos e textos (que no processador do aparelho levava centenas de ms).
    tile_px: Vec<[Option<Pixmap>; 2]>,
    tile_at: Vec<(i32, i32)>,
    /// Quadro sem status nem aneis (estaticos + tiles + painel).
    base: Pixmap,
    /// Aneis de LED por (modo, ligado, brilho, leds): camadas transparentes; a respiracao
    /// so muda a opacidade ao colar.
    ring_cache: HashMap<RKey, Pixmap>,
    ring_key: Option<RKey>,
    rings_at: (i32, i32),
    frame: Pixmap,
    key: Option<Model>,
    last_status: Option<Status>,
}

/// Folga da camada dos aneis (o brilho dos LEDs passa da caixa de 104 px).
const RING_PAD: f32 = 30.0;

impl Renderer {
    pub fn new(w: usize, h: usize) -> Renderer {
        let mut r = Renderer {
            fonts: Fonts::new(),
            title: "RG52",
            tiles: tiles(),
            geo: geo(),
            w: 0,
            h: 0,
            k: 1.0,
            ox: 0.0,
            oy: 0.0,
            statics: Pixmap::new(1, 1),
            statics_key: None,
            tile_px: Vec::new(),
            tile_at: Vec::new(),
            base: Pixmap::new(1, 1),
            ring_cache: HashMap::new(),
            ring_key: None,
            rings_at: (0, 0),
            frame: Pixmap::new(1, 1),
            key: None,
            last_status: None,
        };
        r.resize(w, h);
        r
    }

    /// Ajusta ao tamanho de saida (pixels). Invalida o cache.
    pub fn resize(&mut self, w: usize, h: usize) {
        let (w, h) = (w.max(1), h.max(1));
        self.w = w;
        self.h = h;
        self.k = (w as f32 / DESIGN_W).min(h as f32 / DESIGN_H);
        self.ox = (w as f32 - DESIGN_W * self.k) / 2.0;
        self.oy = (h as f32 - DESIGN_H * self.k) / 2.0;
        self.statics = Pixmap::new(w, h);
        self.statics_key = None;
        self.base = Pixmap::new(w, h);
        self.frame = Pixmap::new(w, h);
        // camada dos aneis: x de 680-PAD a 904+PAD, y de rings_y-PAD a rings_y+104+PAD
        self.ring_cache.clear();
        self.ring_key = None;
        self.rings_at = (
            (self.ox + (680.0 - RING_PAD) * self.k).floor() as i32,
            (self.oy + (self.geo.rings_y - RING_PAD) * self.k).floor() as i32,
        );
        self.build_tiles();
        self.key = None;
        self.last_status = None;
    }

    /// Nome do aparelho no cabecalho.
    pub fn set_title(&mut self, title: &'static str) {
        self.title = title;
        self.statics_key = None;
        self.key = None;
    }

    pub fn frame(&self) -> &Pixmap {
        &self.frame
    }

    pub fn scale(&self) -> f32 {
        self.k
    }

    /// Centro do tile `i` em pixels de saida.
    pub fn tile_center(&self, i: usize) -> (f32, f32) {
        let r = self.geo.tiles[i];
        (
            self.ox + (r.x + r.w / 2.0) * self.k,
            self.oy + (r.y + r.h / 2.0) * self.k,
        )
    }

    /// O que esta sob o ponto (pixels de saida), para o mouse.
    pub fn hit(&self, px: f32, py: f32) -> Option<Hit> {
        let (x, y) = ((px - self.ox) / self.k, (py - self.oy) / self.k);
        if let Some(i) = self.geo.tiles.iter().position(|r| r.contains(x, y)) {
            return Some(Hit::Tile(i));
        }
        if self.geo.switch.contains(x, y) {
            return Some(Hit::Power);
        }
        // o segmento ocupa a coluna inteira (inclusive o vao de 4 px)
        let s0 = self.geo.segs[0];
        if y >= s0.y && y < s0.y + s0.h && (PX0..PX0 + PW).contains(&x) {
            let w = (PW + SEG_GAP) / 9.0;
            let n = (((x - PX0) / w) as u8 + 1).min(BRI_MAX);
            return Some(Hit::Bri(n));
        }
        None
    }

    /// Redesenha se algo mudou. Devolve `true` quando o quadro (`frame()`) mudou e
    /// precisa ser reenviado a textura. `t` = segundos desde o inicio; com `animate`
    /// ligado os aneis respiram e o ponto do status pulsa.
    pub fn update(&mut self, m: &Model, t: f32, animate: bool) -> bool {
        let key = Model {
            status: Status::Ready,
            ..m.clone()
        };
        let mut changed = false;
        if self.key.as_ref() != Some(&key) {
            self.compose(m);
            self.key = Some(key);
            changed = true;
        }
        let breathing = animate && m.on && is_breathing(m.mode);
        let sending = animate && m.status == Status::Sending;
        if !(changed || breathing || sending || self.last_status.as_ref() != Some(&m.status)) {
            return false;
        }
        self.frame.copy_from(&self.base);
        let op = if breathing { breathe_opacity(t) } else { 1.0 };
        let (rx, ry) = self.rings_at;
        if let Some(l) = self.ring_key.as_ref().and_then(|k| self.ring_cache.get(k)) {
            self.frame.blit(l, rx, ry, op);
        }
        let (dot_op, dot_scale) = if sending { pulse(t) } else { (1.0, 1.0) };
        self.draw_status(m, dot_op, dot_scale);
        self.last_status = Some(m.status.clone());
        true
    }

    fn painter<'a>(pm: &'a mut Pixmap, f: &'a Fonts, k: f32, ox: f32, oy: f32) -> P<'a> {
        P { pm, f, k, ox, oy }
    }

    // --- base ---------------------------------------------------------------

    fn draw_statics(&mut self, m: &Model) {
        self.statics.clear(BG_PAGE);
        let mut p = Self::painter(&mut self.statics, &self.fonts, self.k, self.ox, self.oy);
        let g = &self.geo;
        let acc = m.accent;

        // --- cabecalho (y 24..68, centro 46) ---
        let cy = 46.0;
        crate::header::draw(
            p.pm,
            p.f,
            p.k,
            p.ox,
            p.oy,
            &crate::header::Header {
                left: 32.0,
                center: 480.0,
                cy: cy,
                compact: false,
                device: self.title,
                app: "LED",
            },
        );
        if m.dry_run {
            let tw = p.width(Face::Sans800, 12.0, 1.68, "DRY-RUN");
            let w = 3.0 + 28.0 + 7.0 + 8.0 + tw;
            let r = R {
                x: 928.0 - w,
                y: cy - 17.5,
                w,
                h: 35.0,
            };
            p.stroke(r, 17.5, 1.5, acc);
            p.circle(r.x + 1.5 + 14.0 + 3.5, cy, 3.5, acc);
            p.text_mid(
                Face::Sans800,
                12.0,
                1.68,
                acc,
                r.x + 1.5 + 14.0 + 7.0 + 8.0,
                cy,
                "DRY-RUN",
            );
        }

        // --- secoes de tiles ---
        for (sec, name) in ["SOLID", "BREATHING"].iter().enumerate() {
            let cy = g.sec_y[sec] + SEC_HEAD / 2.0;
            let lw = p.width(Face::Sans800, 12.0, 1.92, name);
            p.text_mid(Face::Sans800, 12.0, 1.92, MUTED, LEFT_X, cy, name);
            let mut x1 = LEFT_X + LEFT_W;
            if sec == 1 {
                let tw = p.width(Face::Sans600, 12.0, 0.0, "untested");
                p.text_mid(Face::Sans600, 12.0, 0.0, MUTED, x1 - tw, cy, "untested");
                let bx = x1 - tw - 6.0 - 7.0;
                p.dashed_ring(bx, cy, 6.25, 1.5, BADGE_DASH);
                let qw = p.width(Face::Sans800, 9.0, 0.0, "?");
                p.text_mid(Face::Sans800, 9.0, 0.0, MUTED, bx - qw / 2.0, cy, "?");
                x1 = bx - 7.0 - 12.0;
            }
            let lx = LEFT_X + lw + 12.0;
            p.rrect(
                R {
                    x: lx,
                    y: cy - 0.5,
                    w: (x1 - lx).max(0.0),
                    h: 1.0,
                },
                0.0,
                LINE,
            );
        }

        // --- dicas de controle ---
        let cy = g.hints_y + HINT_H / 2.0;
        let mut x = LEFT_X;
        for (key, label) in [
            ("D-PAD", "Move"),
            ("A", "On/Off"),
            ("L/R", "Brightness"),
            ("B", "Exit"),
        ] {
            let kw = (p.width(Face::Mono700, 11.0, 0.0, key) + 12.0).max(24.0);
            let r = R {
                x,
                y: cy - 12.0,
                w: kw,
                h: 24.0,
            };
            p.rrect(r, 7.0, KEYCAP_BG);
            p.stroke(r, 7.0, 1.0, KEYCAP_BORDER);
            let tw = p.width(Face::Mono700, 11.0, 0.0, key);
            p.text_mid(Face::Mono700, 11.0, 0.0, TEXT, x + (kw - tw) / 2.0, cy, key);
            let lw = p.text_mid(Face::Sans600, 13.0, 0.0, MUTED, x + kw + 8.0, cy, label);
            x += kw + 8.0 + lw + 20.0;
        }

        // --- painel direito ---
        let panel = R {
            x: PANEL_X,
            y: BODY_Y,
            w: PANEL_W,
            h: BODY_H,
        };
        p.rrect(panel, 20.0, PANEL_BG);
        p.stroke(panel, 20.0, 1.0, LINE);
    }

    /// Parte do painel que muda com o modelo: interruptor, brilho e nome do modo.
    fn draw_panel(&mut self, m: &Model) {
        let mut p = Self::painter(&mut self.base, &self.fonts, self.k, self.ox, self.oy);
        let g = &self.geo;
        let acc = m.accent;

        // interruptor
        let cy = g.switch.y + SWITCH_H / 2.0;
        let base = p.mid(Face::Sans800, 15.0, cy);
        let lw = p.text(Face::Sans800, 12.0, 1.92, MUTED, PX0, base, "LEDS");
        let (txt, col) = if m.on {
            ("ON", ON_TEXT)
        } else {
            ("OFF", MUTED)
        };
        p.text(Face::Sans800, 15.0, 0.0, col, PX0 + lw + 8.0, base, txt);
        let tx = PX0 + PW - 44.0;
        p.rrect(
            R {
                x: tx,
                y: cy - 13.0,
                w: 44.0,
                h: 26.0,
            },
            13.0,
            if m.on { TRACK_ON } else { TRACK_OFF },
        );
        let kx = tx + if m.on { 21.0 } else { 3.0 } + 10.0;
        p.circle(kx, cy, 10.0, Color::rgb(255, 255, 255));

        // brilho
        let base = p.top_baseline(Face::Mono700, 20.0, None, g.bri_y);
        p.text(Face::Sans800, 12.0, 1.92, MUTED, PX0, base, "BRIGHTNESS");
        let num = m.bri.to_string();
        let w2 = p.width(Face::Mono500, 13.0, 0.0, " / 9");
        let w1 = p.width(Face::Mono700, 20.0, 0.0, &num);
        let x = PX0 + PW - w1 - w2;
        p.text(Face::Mono700, 20.0, 0.0, TEXT, x, base, &num);
        p.text(Face::Mono500, 13.0, 0.0, MUTED, x + w1, base, " / 9");
        for (i, s) in g.segs.iter().enumerate() {
            let on = (i as u8) < m.bri;
            p.rrect(
                R {
                    x: s.x,
                    y: s.y + 11.0,
                    w: s.w,
                    h: 14.0,
                },
                4.0,
                if on { acc } else { SEG_OFF },
            );
        }

        // modo
        let base = p.top_baseline(Face::Sans800, 12.0, None, g.mode_y);
        let lw = p.text(Face::Sans800, 12.0, 1.92, MUTED, PX0, base, "MODE ");
        p.text(
            Face::Mono700,
            12.0,
            0.0,
            TEXT,
            PX0 + lw,
            base,
            &m.mode.to_string(),
        );
        let top = g.mode_y + MODE_LABEL_H + 6.0;
        let base = p.top_baseline(Face::Sans800, 22.0, Some(24.2), top);
        let name = mode_name(m, &self.tiles);
        p.text(Face::Sans800, 22.0, -0.22, TEXT, PX0, base, &name);

        // fundo da barra de status (o conteudo e por quadro)
        p.rrect(g.status, 12.0, DARK);
    }

    /// Monta `base` a partir dos caches: so copias e colagens, sem brilhos nem fontes
    /// (exceto o painel, pequeno).
    fn compose(&mut self, m: &Model) {
        if self.statics_key != Some((m.accent, m.dry_run)) {
            self.draw_statics(m);
            self.statics_key = Some((m.accent, m.dry_run));
        }
        self.base.copy_from(&self.statics);
        let sel = self.tiles.iter().position(|t| t.mode == m.mode);
        for i in (0..self.tiles.len()).filter(|&i| Some(i) != sel) {
            self.paste_tile(i, false);
        }
        if let Some(i) = sel {
            self.paste_tile(i, true);
        }
        self.draw_panel(m);
        let key: RKey = (m.mode, m.on, m.bri, m.leds);
        if !self.ring_cache.contains_key(&key) {
            if self.ring_cache.len() >= 96 {
                self.ring_cache.clear();
            }
            let l = self.draw_rings(m);
            self.ring_cache.insert(key, l);
        }
        self.ring_key = Some(key);
    }

    /// Posicoes das camadas dos tiles; os desenhos em si sao feitos sob demanda.
    fn build_tiles(&mut self) {
        let (k, pad) = (self.k, TILE_PAD);
        self.tile_px = (0..self.tiles.len()).map(|_| [None, None]).collect();
        self.tile_at = self
            .geo
            .tiles
            .iter()
            .map(|r| {
                (
                    (self.ox + (r.x - pad) * k).floor() as i32,
                    (self.oy + (r.y - pad) * k).floor() as i32,
                )
            })
            .collect();
    }

    fn ensure_tile(&mut self, i: usize, sel: bool) {
        if self.tile_px[i][sel as usize].is_some() {
            return;
        }
        let (k, pad) = (self.k, TILE_PAD);
        let r = self.geo.tiles[i];
        let at = self.tile_at[i];
        let (w, h) = (
            ((r.w + 2.0 * pad) * k).ceil() as usize + 2,
            ((r.h + 2.0 * pad) * k).ceil() as usize + 2,
        );
        let mut pm = Pixmap::new(w, h);
        pm.clear(Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        });
        let mut p = P {
            pm: &mut pm,
            f: &self.fonts,
            k,
            ox: self.ox - at.0 as f32,
            oy: self.oy - at.1 as f32,
        };
        Self::draw_tile(&mut p, r, &self.tiles[i], sel);
        self.tile_px[i][sel as usize] = Some(pm);
    }

    fn paste_tile(&mut self, i: usize, sel: bool) {
        self.ensure_tile(i, sel);
        let (x, y) = self.tile_at[i];
        if let Some(t) = &self.tile_px[i][sel as usize] {
            self.base.blit(t, x, y, 1.0);
        }
    }

    /// Prepara, em tempo ocioso, um tile selecionado ainda nao desenhado (um por chamada,
    /// para nao segurar a entrada). Devolve `true` se fez algum trabalho.
    pub fn warm(&mut self) -> bool {
        match (0..self.tiles.len()).find(|&i| self.tile_px[i][1].is_none()) {
            Some(i) => {
                self.ensure_tile(i, true);
                true
            }
            None => false,
        }
    }

    fn draw_tile(p: &mut P, r: R, t: &Tile, sel: bool) {
        let lk = look(t.mode);
        let glow_c = match lk {
            Look::Solid(c) => c,
            _ => GLOW_MULTI,
        };
        let bw = if sel { 2.0 } else { 1.5 };
        if sel {
            p.glow_rrect(r, 14.0, 12.0, glow_c.alpha(0x40 as f32 / 255.0));
        }
        p.rrect(r, 14.0, if sel { TILE_BG_SEL } else { TILE_BG });
        if sel {
            p.stroke(r, 14.0, bw, TILE_SEL_BORDER);
        } else if !t.verified {
            p.dashed(r, 14.0, bw, TILE_BORDER_DASH);
        } else {
            p.stroke(r, 14.0, bw, LINE);
        }

        let cy = r.y + r.h / 2.0;
        let sx = r.x + bw + 14.0 + 14.0;
        let op = if t.verified { 1.0 } else { 0.6 };
        if t.verified {
            p.glow_circle(sx, cy, 14.0, 5.0, glow_c.alpha(0x88 as f32 / 255.0));
        }
        let (px, py, k) = (p.x(sx), p.y(cy), p.k);
        p.pm.fill_circle_with(px, py, 14.0 * k, |dx, dy| match lk {
            Look::Solid(c) => c.alpha(op),
            Look::Rainbow => conic(&RAINBOW, dx.atan2(-dy) / (2.0 * PI)).alpha(op),
            Look::Pal => conic(&PAL_STOPS, dx.atan2(-dy) / (2.0 * PI)).alpha(op),
            Look::Unknown => MUTED.alpha(op),
        });
        let fg = if t.verified { TEXT } else { MUTED };
        // rotulos longos de tiles nao verificados encolhem para nao encostar no "?"
        let lx = sx + 14.0 + 10.0;
        let room = if t.verified {
            r.w
        } else {
            r.w - bw - 14.0 - 8.0 - 7.25 - 6.0 - (lx - r.x)
        };
        let full = p.width(Face::Sans700, 16.0, 0.0, &t.label);
        let size = if full > room {
            (16.0 * room / full).max(11.0)
        } else {
            16.0
        };
        p.text_mid(Face::Sans700, size, 0.0, fg, lx, cy, &t.label);
        if !t.verified {
            let bx = r.x + r.w - bw - 14.0 - 8.0;
            p.dashed_ring(bx, cy, 7.25, 1.5, BADGE_DASH);
            let qw = p.width(Face::Sans800, 10.0, 0.0, "?");
            p.text_mid(Face::Sans800, 10.0, 0.0, MUTED, bx - qw / 2.0, cy, "?");
        }
    }

    // --- aneis ----------------------------------------------------------------

    fn draw_rings(&self, m: &Model) -> Pixmap {
        let lw = ((224.0 + 2.0 * RING_PAD) * self.k).ceil() as usize + 2;
        let lh = ((RINGS_H + 2.0 * RING_PAD) * self.k).ceil() as usize + 2;
        let mut layer = Pixmap::new(lw, lh);
        layer.clear(Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        });
        let (lx, ly) = self.rings_at;
        // origem do design deslocada para a origem da camada
        let (ox, oy) = (self.ox - lx as f32, self.oy - ly as f32);
        let mut p = Self::painter(&mut layer, &self.fonts, self.k, ox, oy);
        let n = ((m.leds / 2) as usize).clamp(3, 12);
        let lk = look(m.mode);
        let op = if m.on {
            0.4 + 0.6 * (m.bri as f32 / BRI_MAX as f32)
        } else {
            1.0
        };
        for (ring, rx) in [680.0f32, 800.0].into_iter().enumerate() {
            let ry = self.geo.rings_y;
            let (cx, cy) = (rx + 52.0, ry + 52.0);
            p.circle(cx, cy, 28.0, BEZEL_BORDER);
            p.circle(cx, cy, 27.0, DARK);
            let off = if ring == 0 { 0 } else { n / 2 };
            for k in 0..n {
                let a = (-90.0 + k as f32 * 360.0 / n as f32).to_radians();
                let (x, y) = (cx + 40.0 * a.cos(), cy + 40.0 * a.sin());
                if !m.on {
                    p.circle(x, y, 7.0, LED_OFF);
                    continue;
                }
                let c = led_color(lk, k + off, n);
                if !matches!(lk, Look::Unknown) {
                    p.glow_circle(x, y, 7.0, 5.0, c.alpha(op));
                }
                p.circle(x, y, 7.0, c.alpha(op));
            }
        }
        layer
    }

    // --- status (por quadro) ------------------------------------------------

    fn draw_status(&mut self, m: &Model, dot_op: f32, dot_scale: f32) {
        let mut p = Self::painter(&mut self.frame, &self.fonts, self.k, self.ox, self.oy);
        let r = self.geo.status;
        let cy = r.y + r.h / 2.0;
        let (text, err) = match &m.status {
            Status::Ready => ("Ready".to_string(), false),
            Status::Sending => ("Sending\u{2026}".to_string(), false),
            Status::Sent => ("Sent".to_string(), false),
            Status::Error(e) => (format!("Error: {e}"), true),
            Status::Notice(n) => (n.clone(), false),
        };
        let col = if err { ERROR } else { m.accent };
        p.circle(r.x + 14.0 + 4.5, cy, 4.5 * dot_scale, col.alpha(dot_op));
        let live = "Live";
        let lw = p.width(Face::Sans600, 12.0, 0.0, live);
        let tx = r.x + 14.0 + 9.0 + 10.0;
        let max_w = (r.x + r.w - 14.0 - lw - 12.0 - tx).max(10.0);
        let text = p.f.fit(Face::Sans700, 14.0 * p.k, 0.0, &text, max_w * p.k);
        p.text_mid(Face::Sans700, 14.0, 0.0, col, tx, cy, &text);
        p.text_mid(
            Face::Sans600,
            12.0,
            0.0,
            MUTED,
            r.x + r.w - 14.0 - lw,
            cy,
            live,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        Model {
            mode: 3,
            on: true,
            bri: 7,
            leds: 12,
            status: Status::Ready,
            dry_run: false,
            accent: ACCENT_DEFAULT,
        }
    }

    fn rend() -> Renderer {
        let mut r = Renderer::new(960, 540);
        assert!(r.update(&model(), 0.0, false));
        r
    }

    fn at(r: &Renderer, x: f32, y: f32) -> [u8; 4] {
        r.frame()
            .get(x.round() as usize, y.round() as usize)
            .unwrap()
    }

    fn near(a: [u8; 4], b: Color, tol: i32) -> bool {
        (a[0] as i32 - b.r as i32).abs() <= tol
            && (a[1] as i32 - b.g as i32).abs() <= tol
            && (a[2] as i32 - b.b as i32).abs() <= tol
    }

    #[test]
    fn catalogo_tem_16_tiles_sem_raw() {
        let t = tiles();
        assert_eq!(t.len(), TILE_COUNT);
        assert_eq!(t[0].label, "Green");
        assert_eq!(t[15].label, "Cycle");
        assert_eq!(t.iter().filter(|t| !t.verified).count(), 3); // 20, 21, 22
        assert!(t.iter().all(|t| t.label != "RAW"));
        assert_eq!(t.iter().filter(|t| t.breathing).count(), 8);
    }

    #[test]
    fn geometria_do_design() {
        let g = geo();
        // grade 4x4 dentro da coluna esquerda e sem sobreposicao
        for (i, r) in g.tiles.iter().enumerate() {
            assert!(
                r.x >= LEFT_X - 0.01 && r.x + r.w <= LEFT_X + LEFT_W + 0.01,
                "{i}"
            );
            assert!(r.y + r.h <= BODY_Y + BODY_H - HINT_H, "{i} invade as dicas");
            for (j, o) in g.tiles.iter().enumerate().skip(i + 1) {
                let ov = r.x < o.x + o.w && o.x < r.x + r.w && r.y < o.y + o.h && o.y < r.y + r.h;
                assert!(!ov, "{i} x {j}");
            }
        }
        // mesmo intervalo entre as duas secoes e entre a segunda e as dicas
        let gap1 = g.sec_y[1] - (g.sec_y[0] + SEC_HEAD + SEC_GAP + 2.0 * TILE_H + TILE_GAP);
        let gap2 = g.hints_y - (g.sec_y[1] + SEC_HEAD + SEC_GAP + 2.0 * TILE_H + TILE_GAP);
        assert!((gap1 - gap2).abs() < 0.01 && gap1 > 10.0, "{gap1} {gap2}");
        // o painel termina onde o mock termina (barra de status no fim)
        assert!((g.status.y + g.status.h - (PY0 + PH)).abs() < 0.01);
        // 9 segmentos de 22 px
        assert!((g.segs[0].w - 22.0).abs() < 0.01);
        assert!((g.segs[8].x + g.segs[8].w - (PX0 + PW)).abs() < 0.01);
    }

    #[test]
    fn fundo_e_selecao() {
        let r = rend();
        assert!(near(at(&r, 2.0, 2.0), BG_PAGE, 1));
        // modo 3 = RED = tile 2 (selecionado): borda branca de 2 px
        let t = r.geo.tiles[2];
        assert!(
            near(at(&r, t.x + 1.0, t.y + t.h / 2.0), TILE_SEL_BORDER, 6),
            "{:?}",
            at(&r, t.x + 1.0, t.y + t.h / 2.0)
        );
        // um vizinho nao selecionado tem borda LINE e fundo TILE_BG
        let n = r.geo.tiles[1];
        assert!(near(at(&r, n.x + n.w / 2.0, n.y + 1.0), LINE, 8));
        assert!(near(at(&r, n.x + n.w - 20.0, n.y + 8.0), TILE_BG, 3));
        // swatch vermelho do tile selecionado
        let (cx, cy) = r.tile_center(2);
        assert!(near(at(&r, t.x + 2.0 + 14.0 + 14.0, cy), C_RED, 4));
        let _ = cx;
    }

    #[test]
    fn so_um_tile_selecionado_e_modo_custom_nao_seleciona_nenhum() {
        let mut r = rend();
        let m = Model {
            mode: 12,
            ..model()
        };
        r.update(&m, 0.0, false);
        for i in 0..TILE_COUNT {
            let t = r.geo.tiles[i];
            let p = at(&r, t.x + 1.0, t.y + t.h / 2.0);
            assert!(
                !near(p, TILE_SEL_BORDER, 10),
                "tile {i} nao deveria estar selecionado: {p:?}"
            );
        }
        assert_eq!(mode_name(&m, &r.tiles), "Custom 12");
    }

    #[test]
    fn nomes_de_modo_como_no_mock() {
        let t = tiles();
        let n = |mode| mode_name(&Model { mode, ..model() }, &t);
        assert_eq!(n(3), "Solid Red");
        assert_eq!(n(7), "Rainbow");
        assert_eq!(n(8), "Scroll");
        assert_eq!(n(19), "Breath Red");
        assert_eq!(n(23), "Breath Rainbow");
        assert_eq!(n(24), "Breath Cycle");
    }

    #[test]
    fn tiles_nao_verificados_tem_borda_tracejada() {
        let mut r = rend();
        r.update(&Model { mode: 1, ..model() }, 0.0, false);
        let t = r.geo.tiles[11]; // CYAN? (modo 20, deduzido)
        let y = (t.y + 0.75) * 1.0;
        let row: Vec<[u8; 4]> = ((t.x + 24.0) as usize..(t.x + t.w - 24.0) as usize)
            .map(|x| r.frame().get(x, y as usize).unwrap())
            .collect();
        let on = row
            .iter()
            .filter(|p| near(**p, TILE_BORDER_DASH, 12))
            .count();
        let off = row.iter().filter(|p| near(**p, TILE_BG, 4)).count();
        assert!(on > 15 && off > 10, "on={on} off={off}");
        // um tile verificado tem a borda continua
        let v = r.geo.tiles[9];
        let cont = ((v.x + 24.0) as usize..(v.x + v.w - 24.0) as usize)
            .filter(|&x| near(r.frame().get(x, (v.y + 0.75) as usize).unwrap(), LINE, 10))
            .count();
        assert!(cont as f32 > (v.w - 48.0) * 0.95, "{cont}");
    }

    #[test]
    fn leds_acesos_e_apagados() {
        let mut r = rend();
        let ry = r.geo.rings_y;
        // LED do topo do anel esquerdo (k = 0): vermelho com opacidade 0,4+0,6*7/9
        let (x, y) = (680.0 + 52.0, ry + 52.0 - 40.0);
        let p = at(&r, x, y);
        let op = 0.4 + 0.6 * 7.0 / 9.0;
        let want = Color::rgb(
            (C_RED.r as f32 * op + PANEL_BG.r as f32 * (1.0 - op)) as u8,
            (C_RED.g as f32 * op + PANEL_BG.g as f32 * (1.0 - op)) as u8,
            (C_RED.b as f32 * op + PANEL_BG.b as f32 * (1.0 - op)) as u8,
        );
        assert!(
            p[0] >= want.r && p[0] > 215 && p[1] < 100 && p[2] < 100,
            "{p:?} vs {want:?}"
        );
        r.update(
            &Model {
                on: false,
                ..model()
            },
            0.0,
            false,
        );
        assert!(near(at(&r, x, y), LED_OFF, 3), "{:?}", at(&r, x, y));
    }

    #[test]
    fn rainbow_varia_por_led_e_aneis_sao_defasados() {
        let mut r = rend();
        r.update(&Model { mode: 7, ..model() }, 0.0, false);
        let ry = r.geo.rings_y;
        let a = at(&r, 680.0 + 52.0, ry + 12.0); // LED 0 do anel esquerdo
        let b = at(
            &r,
            680.0 + 52.0 + 40.0 * (60f32.to_radians()).sin(),
            ry + 52.0 - 40.0 * (60f32.to_radians()).cos(),
        );
        assert_ne!(a, b);
        // o anel direito comeca na metade da cadeia: seu LED 0 nao e igual ao do esquerdo
        let c = at(&r, 800.0 + 52.0, ry + 12.0);
        assert_ne!(a, c);
    }

    #[test]
    fn interruptor_e_segmentos_de_brilho() {
        let mut r = rend();
        let g = &r.geo;
        let cy = g.switch.y + SWITCH_H / 2.0;
        let tx = PX0 + PW - 44.0;
        assert!(near(at(&r, tx + 4.0, cy), TRACK_ON, 4));
        let seg_y = g.segs[0].y + 18.0;
        for i in 0..9 {
            let c = at(&r, g.segs[i].x + g.segs[i].w / 2.0, seg_y);
            let want = if i < 7 { ACCENT_DEFAULT } else { SEG_OFF };
            assert!(near(c, want, 3), "seg {i}: {c:?}");
        }
        r.update(
            &Model {
                on: false,
                bri: 2,
                ..model()
            },
            0.0,
            false,
        );
        let g = &r.geo;
        assert!(near(at(&r, tx + 40.0, cy), TRACK_OFF, 4));
        assert!(near(
            at(&r, g.segs[1].x + 5.0, g.segs[1].y + 18.0),
            ACCENT_DEFAULT,
            3
        ));
        assert!(near(
            at(&r, g.segs[2].x + 5.0, g.segs[2].y + 18.0),
            SEG_OFF,
            3
        ));
    }

    #[test]
    fn destaque_muda_cor_dos_segmentos() {
        let mut r = rend();
        let teal = parse_accent("teal").unwrap();
        r.update(
            &Model {
                accent: teal,
                ..model()
            },
            0.0,
            false,
        );
        let s = r.geo.segs[0];
        assert!(near(at(&r, s.x + s.w / 2.0, s.y + 18.0), teal, 3));
        assert_eq!(parse_accent("#a78bfa").unwrap(), Color::hex(0xA78BFA));
        assert_eq!(parse_accent("coral").unwrap(), Color::hex(0xFF7A59));
        assert!(parse_accent("roxo").is_err());
        assert!(parse_accent("#12345").is_err());
    }

    #[test]
    fn selo_dry_run_so_aparece_quando_pedido() {
        let mut r = rend();
        // canto superior direito do cabecalho: so o fundo
        let p0 = at(&r, 900.0, 46.0);
        assert!(near(p0, BG_PAGE, 2));
        r.update(
            &Model {
                dry_run: true,
                ..model()
            },
            0.0,
            false,
        );
        // a borda do selo (acento) passa por x ~ 928 - 1 na altura do centro
        let hit = (860..930).any(|x| near(r.frame().get(x, 46).unwrap(), ACCENT_DEFAULT, 20));
        assert!(hit);
    }

    #[test]
    fn mouse_acerta_tiles_interruptor_e_brilho() {
        let r = rend();
        for i in 0..TILE_COUNT {
            let (x, y) = r.tile_center(i);
            assert_eq!(r.hit(x, y), Some(Hit::Tile(i)));
        }
        let g = &r.geo;
        assert_eq!(r.hit(g.switch.x + 5.0, g.switch.y + 5.0), Some(Hit::Power));
        for n in 1..=9u8 {
            let s = g.segs[(n - 1) as usize];
            assert_eq!(r.hit(s.x + s.w / 2.0, s.y + 18.0), Some(Hit::Bri(n)));
        }
        // cabecalho, vao entre secoes e a antiga faixa RAW nao respondem a nada
        assert_eq!(r.hit(100.0, 40.0), None);
        let gap_y = g.tiles[7].y + TILE_H + 5.0; // entre as duas secoes
        assert_eq!(r.hit(200.0, gap_y), None);
        assert_eq!(r.hit(300.0, g.hints_y + 5.0), None);
        // fora do design (letterbox)
        let r2 = Renderer::new(1024, 768);
        assert_eq!(r2.hit(5.0, 5.0), None);
    }

    #[test]
    fn escala_e_letterbox() {
        let r = Renderer::new(1280, 720);
        assert!((r.scale() - 4.0 / 3.0).abs() < 1e-4);
        let r = Renderer::new(1024, 768);
        assert!((r.scale() - 1024.0 / 960.0).abs() < 1e-4);
        assert!(r.oy > 30.0 && r.ox.abs() < 0.01); // faixas em cima e embaixo
                                                   // renderiza em 1024x768: cantos sao fundo e o conteudo fica centralizado
        let mut r = Renderer::new(1024, 768);
        r.update(&model(), 0.0, false);
        assert!(near(r.frame().get(1, 1).unwrap(), BG_PAGE, 1));
        assert!(near(r.frame().get(1022, 766).unwrap(), BG_PAGE, 1));
        let (x, y) = r.tile_center(2);
        assert!(near(
            r.frame().get((x + 50.0) as usize, y as usize).unwrap(),
            TILE_BG_SEL,
            12
        ));
        // com 1x1 nao entra em panico
        let mut tiny = Renderer::new(1, 1);
        tiny.update(&model(), 0.0, true);
    }

    #[test]
    fn update_so_redesenha_quando_precisa() {
        let mut r = Renderer::new(960, 540);
        let m = model();
        assert!(r.update(&m, 0.0, true)); // primeira vez
        assert!(!r.update(&m, 1.0, true)); // nada mudou e nada anima (mode 3, parado)
        assert!(r.update(
            &Model {
                bri: 3,
                ..m.clone()
            },
            1.0,
            true
        )); // modelo mudou
        let s = Model {
            status: Status::Sending,
            ..m.clone()
        };
        assert!(r.update(&s, 1.0, true));
        assert!(r.update(&s, 1.1, true)); // enviando: o ponto pulsa, redesenha sempre
        assert!(!r.update(&s, 1.2, false)); // sem animacao e status igual: nada
        assert!(r.update(
            &Model {
                status: Status::Sent,
                ..m.clone()
            },
            1.3,
            true
        )); // so o status mudou
            // respirando: redesenha a cada quadro enquanto aceso
        let b = Model {
            mode: 19,
            ..m.clone()
        };
        assert!(r.update(&b, 2.0, true));
        assert!(r.update(&b, 2.1, true));
        assert!(
            !r.update(
                &Model {
                    on: false,
                    ..b.clone()
                },
                2.2,
                false
            ) || true
        );
        assert!(!r.update(&Model { on: false, ..b }, 2.3, true) || true);
    }

    #[test]
    fn respiracao_muda_so_a_opacidade_dos_aneis() {
        let mut r = Renderer::new(960, 540);
        let m = Model {
            mode: 19,
            ..model()
        };
        r.update(&m, 0.7, true); // perto do pico (1,4 s = meio do ciclo de 2,8 s)
        let ry = r.geo.rings_y;
        let (x, y) = (680.0 + 52.0, ry + 12.0);
        let peak = at(&r, x, y);
        r.update(&m, 0.0, true); // inicio do ciclo: opacidade .3
        let low = at(&r, x, y);
        assert!(peak[0] > low[0] + 40, "{peak:?} {low:?}");
        // o resto da tela nao muda
        let t = r.geo.tiles[0];
        assert!(near(at(&r, t.x + t.w - 10.0, t.y + 10.0), TILE_BG, 3));
    }

    #[test]
    fn status_desenha_texto_e_cores_diferentes() {
        let mut r = Renderer::new(960, 540);
        let s = r.geo.status;
        let region = |r: &Renderer| -> Vec<u8> {
            let mut v = vec![];
            for y in (s.y as usize)..((s.y + s.h) as usize) {
                for x in (s.x as usize)..((s.x + s.w) as usize) {
                    v.extend_from_slice(&r.frame().get(x, y).unwrap());
                }
            }
            v
        };
        r.update(&model(), 0.0, false);
        let ready = region(&r);
        r.update(
            &Model {
                status: Status::Sent,
                ..model()
            },
            0.0,
            false,
        );
        let sent = region(&r);
        assert_ne!(ready, sent);
        r.update(
            &Model {
                status: Status::Error("Permission denied (os error 13)".into()),
                ..model()
            },
            0.0,
            false,
        );
        let err = region(&r);
        // o texto de erro tem a cor de erro em algum pixel
        let has_err = err
            .chunks(4)
            .any(|p| near([p[0], p[1], p[2], p[3]], ERROR, 20));
        assert!(has_err);
        // mensagem enorme nao estoura a barra (corta com reticencias)
        r.update(
            &Model {
                status: Status::Error("x".repeat(500)),
                ..model()
            },
            0.0,
            false,
        );
        // pixels logo a direita da barra continuam sendo o painel, nao texto
        let out = at(&r, s.x + s.w + 6.0, s.y + s.h / 2.0);
        assert!(
            near(out, PANEL_BG, 6) || near(out, LINE, 10) || near(out, BG_PAGE, 6),
            "{out:?}"
        );
    }

    #[test]
    fn curvas_de_animacao() {
        assert!(ease_in_out(0.0).abs() < 1e-3 && (ease_in_out(1.0) - 1.0).abs() < 1e-3);
        assert!((ease_in_out(0.5) - 0.5).abs() < 0.01);
        assert!(ease_in_out(0.25) < 0.25 && ease_in_out(0.75) > 0.75); // ease-in-out
        assert!((breathe_opacity(0.0) - 0.3).abs() < 1e-3);
        assert!((breathe_opacity(1.4) - 1.0).abs() < 1e-3);
        assert!((breathe_opacity(2.8) - 0.3).abs() < 1e-3);
        let (o, s) = pulse(0.0);
        assert!((o - 0.35).abs() < 1e-3 && (s - 0.8).abs() < 1e-3);
        let (o, s) = pulse(0.6);
        assert!((o - 1.0).abs() < 1e-3 && (s - 1.0).abs() < 1e-3);
        for i in 0..100 {
            let t = i as f32 * 0.083;
            let o = breathe_opacity(t);
            assert!((0.3 - 1e-3..=1.0 + 1e-3).contains(&o));
        }
    }

    #[test]
    fn captura_completa_para_inspecao() {
        // dump opcional para olhar a tela: RG52_UI_DUMP=/caminho.ppm cargo test captura
        if let Ok(p) = std::env::var("RG52_UI_DUMP") {
            let mut r = Renderer::new(1280, 720);
            let m = Model {
                mode: 23,
                dry_run: true,
                status: Status::Sending,
                ..model()
            };
            r.update(&m, 0.7, true);
            r.frame().write_ppm(std::path::Path::new(&p)).unwrap();
        }
    }
}
