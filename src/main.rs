//! rg52-led: troca as cores dos LEDs dos analogicos do RG52 Mini em tempo real.
//!
//! A tela (design 960x540, ver `ui.rs`) e desenhada por software em um `Pixmap` e
//! enviada a uma textura SDL. A cada movimento no D-pad a cor/efeito e enviado a
//! MCU; envios em rajada sao coalescidos pela thread de LED, entao a interface
//! nunca trava mesmo com o atraso do protocolo.

use std::{
    env,
    path::{Path, PathBuf},
    process,
    time::{Duration, Instant},
};

use rg52led::{
    devices::{self, screen, Feature},
    ui::{parse_accent, tiles, Hit, Model, Renderer, Status, ACCENT_DEFAULT, COLS, TILE_COUNT},
    Color, Config, Controller, Desired, DryRun, LedHandle, Options, Timing, Transport,
    UartTransport, BRI_MAX, BRI_MIN, DEFAULT_DEV, DEFAULT_PWR,
};
use sdl2::{
    controller::{Button, GameController},
    event::{Event, WindowEvent},
    keyboard::Keycode,
    mouse::MouseButton,
    pixels::PixelFormatEnum,
};

// ---------------------------------------------------------------------------
// Argumentos
// ---------------------------------------------------------------------------

struct Args {
    device: Option<String>,
    dev: String,
    pwr: String,
    cfg: PathBuf,
    dry_run: bool,
    fullscreen: bool,
    repeat: usize,
    always_init: bool,
    leds: Option<u32>,
    screenshot: Option<PathBuf>,
    cursor: Option<usize>,
    no_gamepad: bool,
    accent: Color,
    animate: bool,
}

const HELP: &str = "\
rg52-led - cores dos LEDs dos analogicos do RG52 Mini (precisa de root para a serial/debugfs)

USO: rg52-led [opcoes]
  --device ID       auto (padrao), rg52mini, rg43, r36s, rgb10, a10mini, rk3566 ou generic
  --dev PATH        UART da MCU de LED            (padrao /dev/ttyS1)
  --pwr PATH        enable do regulador vcc-led   (padrao /sys/kernel/debug/regulator/vcc-led/enable)
  --cfg PATH        arquivo led.cfg               (padrao /home/ark/.config/led.cfg ou ~/.config/led.cfg)
  --leds N          quantidade de LEDs da cadeia  (padrao: LEDCOUNT do led.cfg, 12)
  --repeat N        repeticoes de cada byte       (padrao 6, como o ledctl.sh)
  --always-init     reenviar o handshake em todo envio (como o ledctl.sh faz)
  --dry-run         nao toca no hardware; mostra os bytes em stderr
  --fullscreen      tela cheia (use no aparelho)
  --no-gamepad      ignora o gamepad do SDL (use quando o gptokeyb ja converte em teclado)
  --accent COR      destaque: amber (padrao), teal, violet, coral ou #RRGGBB
  --no-anim         sem animacoes (respiracao dos aneis, ponto do status)
  --screenshot PPM  (depuracao) desenha um quadro 1280x720 sem abrir janela, salva em PPM e sai
  --cursor N        (depuracao) tile inicial 0..=15
  -h, --help        esta ajuda
";

fn default_cfg() -> PathBuf {
    if Path::new("/home/ark/.config").is_dir() {
        return PathBuf::from("/home/ark/.config/led.cfg");
    }
    match env::var_os("HOME") {
        Some(h) => PathBuf::from(h).join(".config/led.cfg"),
        None => PathBuf::from("led.cfg"),
    }
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        device: None,
        dev: DEFAULT_DEV.into(),
        pwr: DEFAULT_PWR.into(),
        cfg: default_cfg(),
        dry_run: false,
        fullscreen: false,
        repeat: 6,
        always_init: false,
        leds: None,
        screenshot: None,
        cursor: None,
        no_gamepad: false,
        accent: ACCENT_DEFAULT,
        animate: true,
    };
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |name: &str| {
            it.next()
                .ok_or_else(|| format!("{name} precisa de um valor"))
        };
        match arg.as_str() {
            "--device" => a.device = Some(val("--device")?),
            "--dev" => a.dev = val("--dev")?,
            "--pwr" => a.pwr = val("--pwr")?,
            "--cfg" => a.cfg = PathBuf::from(val("--cfg")?),
            "--leds" => a.leds = Some(val("--leds")?.parse().map_err(|_| "--leds invalido")?),
            "--repeat" => a.repeat = val("--repeat")?.parse().map_err(|_| "--repeat invalido")?,
            "--screenshot" => a.screenshot = Some(PathBuf::from(val("--screenshot")?)),
            "--cursor" => {
                let n: usize = val("--cursor")?.parse().map_err(|_| "--cursor invalido")?;
                a.cursor = Some(n.min(TILE_COUNT - 1));
            }
            "--accent" => a.accent = parse_accent(&val("--accent")?)?,
            "--no-anim" => a.animate = false,
            "--always-init" => a.always_init = true,
            "--no-gamepad" => a.no_gamepad = true,
            "--dry-run" => a.dry_run = true,
            "--fullscreen" => a.fullscreen = true,
            "-h" | "--help" => {
                print!("{HELP}");
                process::exit(0);
            }
            other => return Err(format!("opcao desconhecida: {other}\n\n{HELP}")),
        }
    }
    Ok(a)
}

// ---------------------------------------------------------------------------
// Entrada
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    BriUp,
    BriDown,
}

fn key_action(k: Keycode) -> Option<Action> {
    Some(match k {
        Keycode::Up => Action::Up,
        Keycode::Down => Action::Down,
        Keycode::Left => Action::Left,
        Keycode::Right => Action::Right,
        Keycode::Return | Keycode::KpEnter | Keycode::Space => Action::Confirm,
        Keycode::Escape | Keycode::Backspace | Keycode::Q => Action::Back,
        Keycode::RightBracket | Keycode::PageUp | Keycode::Equals | Keycode::KpPlus => {
            Action::BriUp
        }
        Keycode::LeftBracket | Keycode::PageDown | Keycode::Minus | Keycode::KpMinus => {
            Action::BriDown
        }
        _ => return None,
    })
}

fn pad_action(b: Button) -> Option<Action> {
    Some(match b {
        Button::DPadUp => Action::Up,
        Button::DPadDown => Action::Down,
        Button::DPadLeft => Action::Left,
        Button::DPadRight => Action::Right,
        Button::A => Action::Confirm,
        Button::B | Button::Start => Action::Back,
        Button::RightShoulder => Action::BriUp,
        Button::LeftShoulder => Action::BriDown,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Estado
// ---------------------------------------------------------------------------

struct App {
    cfg: Config,
    cfg_path: PathBuf,
    /// Tile com o cursor (0..16): linhas 0-1 = SOLID, 2-3 = BREATHING.
    cur: usize,
    tiles: Vec<rg52led::ui::Tile>,
    led: LedHandle,
    /// Ja mandou algo a MCU nesta sessao (para mostrar "Sent").
    pushed: bool,
    /// Adia o envio a MCU ate depois do proximo quadro: a tela responde primeiro e o LED
    /// em seguida (o main liga isto; os testes enviam na hora).
    defer: bool,
    pending: bool,
    dirty: Option<Instant>,
    hint: Option<(String, Instant)>,
    quit: bool,
}

impl App {
    fn new(cfg: Config, cfg_path: PathBuf, led: LedHandle) -> App {
        let tiles = tiles();
        let cur = tiles.iter().position(|t| t.mode == cfg.mode).unwrap_or(2);
        let mut app = App {
            cfg,
            cfg_path,
            cur,
            tiles,
            led,
            pushed: false,
            defer: false,
            pending: false,
            dirty: None,
            hint: None,
            quit: false,
        };
        // Se estava ligado na ultima vez, sincroniza o hardware com o led.cfg.
        app.push_if_on();
        app
    }

    fn desired(&self) -> Desired {
        Desired {
            enabled: self.cfg.enabled,
            mode: self.cfg.mode,
            bri: self.cfg.bri,
        }
    }

    fn push(&mut self) {
        self.pushed = true;
        if self.defer {
            self.pending = true;
        } else {
            self.led.set(self.desired());
        }
    }

    /// Envia o estado atual a MCU se houve mudanca adiada.
    fn send_pending(&mut self) {
        if std::mem::take(&mut self.pending) {
            self.led.set(self.desired());
        }
    }

    fn push_if_on(&mut self) {
        if self.cfg.enabled {
            self.push();
        }
    }

    fn touch(&mut self) {
        self.dirty = Some(Instant::now());
    }

    fn say(&mut self, s: &str) {
        self.hint = Some((s.to_string(), Instant::now()));
    }

    fn select(&mut self, i: usize) {
        self.cur = i;
        self.cfg.mode = self.tiles[i].mode;
        self.touch();
        self.push_if_on();
    }

    fn toggle(&mut self) {
        self.cfg.enabled = !self.cfg.enabled;
        self.touch();
        self.push();
    }

    fn set_bri(&mut self, b: u8) {
        self.cfg.bri = b.clamp(BRI_MIN, BRI_MAX);
        self.touch();
        self.push_if_on();
    }

    // `% COLS` de proposito: nada de APIs novas da std (o rustc do aparelho e antigo).
    fn act(&mut self, a: Action) {
        let col = self.cur % COLS;
        match a {
            Action::Back => self.quit = true,
            Action::Left if col > 0 => self.select(self.cur - 1),
            Action::Right if col + 1 < COLS => self.select(self.cur + 1),
            Action::Up if self.cur >= COLS => self.select(self.cur - COLS),
            Action::Down if self.cur + COLS < TILE_COUNT => self.select(self.cur + COLS),
            Action::Left | Action::Right | Action::Up | Action::Down => {}
            Action::Confirm => self.toggle(),
            Action::BriUp => self.set_bri(self.cfg.bri.saturating_add(1)),
            Action::BriDown => self.set_bri(self.cfg.bri.saturating_sub(1)),
        }
    }

    fn click(&mut self, h: Hit) {
        match h {
            Hit::Tile(i) => self.select(i),
            Hit::Power => self.toggle(),
            Hit::Bri(n) => self.set_bri(n),
        }
    }

    /// Grava o led.cfg ~0,4 s depois da ultima mudanca (evita escrever a cada passo).
    fn tick(&mut self) {
        if let Some(t) = self.dirty {
            if t.elapsed() > Duration::from_millis(400) {
                self.flush();
            }
        }
        if let Some((_, t)) = &self.hint {
            if t.elapsed() > Duration::from_secs(4) {
                self.hint = None;
            }
        }
    }

    fn flush(&mut self) {
        self.dirty = None;
        if let Err(e) = self.cfg.save(&self.cfg_path) {
            let msg = format!("Cannot save {}: {e}", self.cfg_path.display());
            eprintln!("{msg}");
            self.say(&msg);
        }
    }

    fn status(&self) -> Status {
        if let Some(e) = self.led.error() {
            Status::Error(e)
        } else if self.led.busy() {
            Status::Sending
        } else if let Some((h, _)) = &self.hint {
            Status::Notice(h.clone())
        } else if self.pushed {
            Status::Sent
        } else {
            Status::Ready
        }
    }

    fn model(&self, leds: u32, dry_run: bool, accent: Color) -> Model {
        Model {
            mode: self.cfg.mode,
            on: self.cfg.enabled,
            bri: self.cfg.bri,
            leds,
            status: self.status(),
            dry_run,
            accent,
        }
    }

    fn finish(mut self) {
        if self.dirty.is_some() {
            self.flush();
        }
        // Envia o que ainda estiver pendente e encerra a thread; os LEDs ficam como estao.
        self.led.shutdown();
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            process::exit(2);
        }
    };
    if let Err(e) = run(args) {
        eprintln!("erro: {e}");
        process::exit(1);
    }
}

const INCOMPATIBLE: &str = "This device has no compatible RGB LED controller.";

fn run(args: Args) -> Result<(), String> {
    // Antes de criar o transporte: em outro aparelho a UART1 pode ser outra coisa.
    let detected = devices::resolve(args.device.as_deref())?;
    let simulated = args.dry_run && args.device.is_none();
    if !detected.supports(Feature::RgbLed) && !simulated {
        eprintln!("device incompatible: {}", detected.label());
        if let Some(path) = &args.screenshot {
            let mut pm = rg52led::gfx::Pixmap::new(1280, 720);
            let fonts = rg52led::gfx::Fonts::new();
            screen::render(
                &mut pm,
                &fonts,
                "LED RGB",
                &detected,
                INCOMPATIBLE,
                args.accent,
            );
            return pm.write_ppm(path).map_err(|e| e.to_string());
        }
        return screen::show(
            "LED RGB",
            &detected,
            INCOMPATIBLE,
            args.accent,
            args.fullscreen,
            args.no_gamepad,
        );
    }
    let title = if detected.profile.rgb_led {
        detected.profile.title
    } else {
        "RG52"
    };
    let cfg = Config::load(&args.cfg);
    let leds = args.leds.unwrap_or(cfg.ledcount);
    let opt = Options {
        repeat: args.repeat,
        always_init: args.always_init,
        ledcount: leds,
        timing: Timing::default(),
    };
    let transport: Box<dyn Transport> = if args.dry_run {
        Box::new(DryRun::verbose())
    } else {
        Box::new(UartTransport::new(&args.dev, &args.pwr))
    };
    let led = LedHandle::spawn(Controller::new(transport, opt));
    let mut app = App::new(cfg, args.cfg.clone(), led);
    app.defer = true;
    if let Some(i) = args.cursor {
        app.cur = i;
        app.cfg.mode = app.tiles[i].mode;
    }

    // Captura sem janela: nao precisa de SDL nem de video.
    if let Some(path) = &args.screenshot {
        let mut r = Renderer::new(1280, 720);
        r.set_title(title);
        r.update(
            &app.model(leds, args.dry_run, args.accent),
            0.7,
            args.animate,
        );
        r.frame().write_ppm(path).map_err(|e| e.to_string())?;
        app.finish();
        return Ok(());
    }

    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let gcs = sdl.game_controller()?;
    let mut pads: Vec<GameController> = Vec::new();
    for i in 0..gcs.num_joysticks()? {
        if !args.no_gamepad && gcs.is_game_controller(i) {
            if let Ok(p) = gcs.open(i) {
                pads.push(p);
            }
        }
    }

    let mut wb = video.window(&format!("{title} LED"), 960, 540);
    wb.position_centered().resizable();
    if args.fullscreen {
        wb.fullscreen_desktop();
    }
    let window = wb.build().map_err(|e| e.to_string())?;
    let mut canvas = window
        .into_canvas()
        .present_vsync()
        .build()
        .map_err(|e| e.to_string())?;
    if args.fullscreen {
        sdl.mouse().show_cursor(false);
    }
    let tc = canvas.texture_creator();
    let mut pump = sdl.event_pump()?;

    // Mostra o fundo na hora: a janela abre instantaneamente e a interface vem em seguida
    // (montar o primeiro quadro leva um tempo no processador do aparelho).
    canvas.set_draw_color(sdl2::pixels::Color::RGB(0x0B, 0x0D, 0x12));
    canvas.clear();
    canvas.present();
    let (mut w, mut h) = canvas.output_size()?;
    let mut rend = Renderer::new(w as usize, h as usize);
    rend.set_title(title);
    let mut tex = tc
        .create_texture_streaming(PixelFormatEnum::ARGB8888, w, h)
        .map_err(|e| e.to_string())?;

    let start = Instant::now();
    let mut force = true;
    while !app.quit {
        for ev in pump.poll_iter() {
            match ev {
                Event::Quit { .. } => app.quit = true,
                Event::KeyDown {
                    keycode: Some(k), ..
                } => {
                    if let Some(a) = key_action(k) {
                        app.act(a);
                    }
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    // coordenadas de janela x pixels de saida (HiDPI)
                    let (ww, wh) = canvas.window().size();
                    let (sx, sy) = (w as f32 / ww.max(1) as f32, h as f32 / wh.max(1) as f32);
                    if let Some(hit) = rend.hit(x as f32 * sx, y as f32 * sy) {
                        app.click(hit);
                    }
                }
                Event::Window {
                    win_event:
                        WindowEvent::SizeChanged(..)
                        | WindowEvent::Exposed
                        | WindowEvent::Restored
                        | WindowEvent::Shown,
                    ..
                } => force = true,
                Event::ControllerButtonDown { button, .. } if !args.no_gamepad => {
                    if let Some(a) = pad_action(button) {
                        app.act(a);
                    }
                }
                Event::ControllerDeviceAdded { which, .. } if !args.no_gamepad => {
                    if let Ok(p) = gcs.open(which) {
                        pads.push(p);
                    }
                }
                Event::ControllerDeviceRemoved { which, .. } => {
                    pads.retain(|p| p.instance_id() != which);
                }
                _ => {}
            }
        }
        app.tick();

        let (nw, nh) = canvas.output_size()?;
        if (nw, nh) != (w, h) {
            (w, h) = (nw, nh);
            rend.resize(w as usize, h as usize);
            tex = tc
                .create_texture_streaming(PixelFormatEnum::ARGB8888, w, h)
                .map_err(|e| e.to_string())?;
            force = true;
        }
        let model = app.model(leds, args.dry_run, args.accent);
        let t = start.elapsed().as_secs_f32();
        let changed = rend.update(&model, t, args.animate);
        if changed || force {
            let f = rend.frame();
            tex.update(None, f.data(), f.pitch())
                .map_err(|e| e.to_string())?;
            canvas.copy(&tex, None, None)?;
            canvas.present();
            force = false;
        }
        // So agora o LED: o seletor ja apareceu na tela.
        app.send_pending();
        if !changed && !force && !rend.warm() {
            std::thread::sleep(Duration::from_millis(8));
        }
    }
    app.send_pending();
    app.finish();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rg52led::Event as Ev;
    use std::sync::{Arc, Mutex};

    fn app_with(cfg: Config, name: &str) -> (App, Arc<Mutex<Vec<Ev>>>, PathBuf) {
        let (t, log) = DryRun::recorder();
        let opt = Options {
            repeat: 1,
            timing: Timing::none(),
            ..Options::default()
        };
        let led = LedHandle::spawn(Controller::new(t, opt));
        let path = env::temp_dir().join(format!("rg52led-{name}-{}.cfg", process::id()));
        (App::new(cfg, path.clone(), led), log, path)
    }

    fn cfg(enabled: bool, mode: u8, bri: u8) -> Config {
        Config {
            enabled,
            mode,
            bri,
            ledcount: 12,
        }
    }

    fn settle(a: &App) {
        assert!(a.led.wait_idle(Duration::from_secs(3)));
    }

    fn bytes(log: &Arc<Mutex<Vec<Ev>>>) -> Vec<u8> {
        log.lock()
            .unwrap()
            .iter()
            .filter_map(|e| if let Ev::Byte(b) = e { Some(*b) } else { None })
            .collect()
    }

    #[test]
    fn ligado_no_boot_sincroniza_o_hardware() {
        let (a, log, _) = app_with(cfg(true, 3, 5), "boot");
        settle(&a);
        assert_eq!(bytes(&log), [0x22, 3, 45]);
        assert_eq!(a.cur, 2); // tile RED
    }

    #[test]
    fn desligado_no_boot_nao_toca_no_hardware() {
        let (a, log, _) = app_with(cfg(false, 3, 5), "boot-off");
        settle(&a);
        assert!(log.lock().unwrap().is_empty());
        assert_eq!(a.status(), Status::Ready);
    }

    #[test]
    fn mover_o_cursor_troca_a_cor_ao_vivo() {
        let (mut a, log, _) = app_with(cfg(true, 3, 5), "live");
        a.act(Action::Right); // RED -> CYAN
        assert_eq!((a.cur, a.cfg.mode), (3, 4));
        settle(&a);
        assert_eq!(*bytes(&log).last().unwrap(), 45); // brilho depois do modo
        assert!(bytes(&log).contains(&4));
        a.act(Action::Down); // CYAN (linha 0, col 3) -> SCROLL (linha 1, col 3)
        assert_eq!(a.cur, 7);
        assert_eq!(a.cfg.mode, 8);
        settle(&a);
        assert_eq!(a.status(), Status::Sent);
    }

    #[test]
    fn limites_da_grade_4x4_sem_linha_raw() {
        let (mut a, log, _) = app_with(cfg(false, 1, 5), "bounds");
        assert_eq!(a.cur, 0);
        a.act(Action::Left);
        a.act(Action::Up);
        assert_eq!(a.cur, 0);
        for _ in 0..10 {
            a.act(Action::Right);
        }
        assert_eq!(a.cur, 3);
        for _ in 0..10 {
            a.act(Action::Down);
        }
        // para na ultima linha de BREATHING: nao existe mais nada abaixo
        assert_eq!(a.cur, 15);
        a.act(Action::Down);
        assert_eq!(a.cur, 15);
        a.act(Action::Up);
        assert_eq!(a.cur, 11);
        settle(&a);
        assert!(log.lock().unwrap().is_empty()); // LEDs apagados: so navega
    }

    #[test]
    fn mudar_tile_com_leds_apagados_nao_envia_nada() {
        let (mut a, log, _) = app_with(cfg(false, 3, 5), "off-nav");
        a.act(Action::Right);
        a.act(Action::Down);
        settle(&a);
        assert!(log.lock().unwrap().is_empty());
        assert_eq!(a.cfg.mode, 8);
    }

    #[test]
    fn confirmar_liga_e_desliga() {
        let (mut a, log, _) = app_with(cfg(false, 3, 5), "toggle");
        a.act(Action::Confirm);
        settle(&a);
        assert!(a.cfg.enabled);
        assert_eq!(bytes(&log), [0x22, 3, 45]);
        log.lock().unwrap().clear();
        a.act(Action::Confirm);
        settle(&a);
        assert!(!a.cfg.enabled);
        assert_eq!(bytes(&log), [9]);
        assert_eq!(log.lock().unwrap().last(), Some(&Ev::Power(false)));
    }

    #[test]
    fn brilho_fica_entre_1_e_9() {
        let (mut a, log, _) = app_with(cfg(true, 3, 8), "bri");
        settle(&a);
        log.lock().unwrap().clear();
        for _ in 0..3 {
            a.act(Action::BriUp);
        }
        assert_eq!(a.cfg.bri, 9);
        settle(&a);
        assert_eq!(*bytes(&log).last().unwrap(), 41); // 50 - 9
        for _ in 0..20 {
            a.act(Action::BriDown);
        }
        assert_eq!(a.cfg.bri, 1);
        settle(&a);
        assert_eq!(*bytes(&log).last().unwrap(), 49); // 50 - 1
    }

    #[test]
    fn cliques_do_mouse_equivalem_ao_controle() {
        let (mut a, log, _) = app_with(cfg(true, 3, 5), "mouse");
        settle(&a);
        log.lock().unwrap().clear();
        a.click(Hit::Tile(9)); // BREATHING, 2a linha... modo 18? (17 + 1)
        assert_eq!((a.cur, a.cfg.mode), (9, a.tiles[9].mode));
        a.click(Hit::Bri(9));
        assert_eq!(a.cfg.bri, 9);
        a.click(Hit::Power);
        assert!(!a.cfg.enabled);
        settle(&a);
        assert_eq!(*bytes(&log).last().unwrap(), 9);
    }

    #[test]
    fn modo_desconhecido_no_cfg_nao_quebra() {
        let (a, _, _) = app_with(cfg(false, 12, 5), "unknown");
        assert_eq!(a.cur, 2); // cursor padrao; o modo salvo e preservado
        assert_eq!(a.cfg.mode, 12);
    }

    #[test]
    fn envio_adiado_so_sai_em_send_pending() {
        let (mut a, log, _) = app_with(cfg(true, 3, 5), "defer");
        settle(&a);
        log.lock().unwrap().clear();
        a.defer = true;
        a.act(Action::Left);
        a.act(Action::Left); // rajada: dois movimentos antes do quadro
        settle(&a);
        assert!(
            log.lock().unwrap().is_empty(),
            "nada pode sair antes do quadro"
        );
        a.send_pending();
        settle(&a);
        assert!(bytes(&log).contains(&1)); // so o estado final (GREEN = 1) segue
        assert!(!bytes(&log).contains(&2));
        a.send_pending(); // sem mudanca nova: nao reenvia
        settle(&a);
        let n = bytes(&log).len();
        a.send_pending();
        settle(&a);
        assert_eq!(bytes(&log).len(), n);
    }

    #[test]
    fn config_e_gravada_no_flush() {
        let (mut a, _, path) = app_with(cfg(false, 3, 5), "save");
        a.act(Action::Right);
        a.act(Action::BriUp);
        a.act(Action::Confirm); // liga
        a.flush();
        let saved = Config::load(&path);
        assert_eq!(
            saved,
            Config {
                enabled: true,
                mode: 4,
                bri: 6,
                ledcount: 12
            }
        );
        let _ = std::fs::remove_file(path);
    }
}
