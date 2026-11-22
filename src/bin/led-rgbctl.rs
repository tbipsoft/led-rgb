//! led-rgbctl: testes por linha de comando dos LEDs dos analogicos do RG52 Mini.
//!
//! Serve para (1) trocar cor/brilho sem a interface grafica e (2) explorar a MCU:
//! enviar bytes crus e varrer faixas de bytes anotando o que cada um faz.
//! Nao depende do SDL2. Precisa de root (UART e debugfs), exceto com --dry-run.

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{self, BufRead, Write},
    path::PathBuf,
    process, thread,
    time::Duration,
};

use led_rgb::{
    bri_byte,
    devices::{self, Feature},
    init_byte, parse_byte, Config, DryRun, Options, Session, Timing, Transport, UartTransport,
    BREATHING, BRI_MAX, BRI_MIN, DEFAULT_DEV, DEFAULT_PWR, MODE_OFF, SOLID,
};

const HELP: &str = "\
led-rgbctl - testes por linha de comando dos LEDs do RG52 Mini
(precisa de root; com --dry-run mostra os bytes sem tocar no hardware)

USO: led-rgbctl [opcoes] <comando> [argumentos]

COMANDOS
  color NOME [--breath]   red green blue cyan yellow magenta rainbow scroll
                          (cycle = scroll respirando; --breath soma 16 ao modo)
  mode N                  envia o byte de modo N (0..255 ou 0x..), sem validar: serve para explorar
  bri N                   muda so o brilho (1..9), mantendo o modo do led.cfg (ou --mode)
  sweep                   varre o brilho de --from a --to (padrao 1..9), --delay s por passo (1.5)
  scan                    percorre bytes e deixa voce anotar o que ve nos LEDs:
                            --kind mode (padrao): bytes 1..31 na posicao do MODO
                            --kind bri: bytes 36..60 na posicao do BRILHO (modo fixo em --mode)
                          Enter=proximo  b=voltar  r=reenviar  g N=ir para N  q=sair
                          qualquer outro texto = anotacao para o byte atual
  raw BYTE...             envia bytes crus, sem init (--init envia o handshake antes)
  off                     apaga (byte 9) e corta o trilho
  status                  mostra o led.cfg e os bytes calculados (nao toca no hardware)

OPCOES
  --bri N | --bri-raw B   brilho 1..9, ou um byte cru no lugar do brilho (explorar alem de 1..9)
  --mode N                modo base para bri / sweep / scan --kind bri (padrao: MODE do led.cfg)
  --from N --to N         faixa do sweep (brilho) ou do scan (bytes)
  --auto S                scan sem perguntar: avanca sozinho a cada S segundos
  --resend-bri            scan --kind mode: reenvia o brilho depois de cada modo
  --out ARQUIVO           acrescenta as anotacoes do scan neste arquivo
  --save                  grava modo/brilho no led.cfg (ligado) depois de color / mode / bri
  --repeat N              repeticoes de cada byte (padrao 6; 1 e bem mais rapido)
  --leds N                quantidade de LEDs (padrao: LEDCOUNT do led.cfg, 12)
  --dev PATH  --pwr PATH  --cfg PATH
  --dry-run               nao toca no hardware; imprime os eventos em stderr

EXEMPLOS
  led-rgbctl color red --bri 9
  led-rgbctl color rainbow --breath --save
  led-rgbctl mode 12                 # testa um modo que nao esta no menu
  led-rgbctl sweep --mode 3          # ve os 9 niveis de brilho no vermelho
  led-rgbctl scan --out modos.txt    # varre os modos 1..31 anotando o que cada um faz
  led-rgbctl scan --kind bri --mode 7 --from 36 --to 60
  led-rgbctl raw --init 3 44         # modo 3 e brilho com byte cru 44
";

const VALUE_FLAGS: &[&str] = &[
    "dev", "pwr", "cfg", "leds", "repeat", "bri", "bri-raw", "mode", "from", "to", "auto", "out",
    "kind", "delay",
];
const BOOL_FLAGS: &[&str] = &["dry-run", "save", "resend-bri", "breath", "init", "help"];

type Sess = Session<Box<dyn Transport>>;

// ---------------------------------------------------------------------------
// Argumentos
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Parsed {
    pos: Vec<String>,
    vals: HashMap<String, String>,
    flags: HashSet<String>,
}

impl Parsed {
    fn flag(&self, k: &str) -> bool {
        self.flags.contains(k)
    }
    fn val(&self, k: &str) -> Option<&str> {
        self.vals.get(k).map(|s| s.as_str())
    }
}

fn parse_args() -> Result<Parsed, String> {
    let mut p = Parsed::default();
    let mut it = env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "-h" {
            p.flags.insert("help".into());
            continue;
        }
        let Some(rest) = a.strip_prefix("--") else {
            p.pos.push(a);
            continue;
        };
        let (name, inline) = match rest.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (rest, None),
        };
        if BOOL_FLAGS.contains(&name) {
            p.flags.insert(name.to_string());
        } else if VALUE_FLAGS.contains(&name) {
            let v = match inline {
                Some(v) => v,
                None => it
                    .next()
                    .ok_or_else(|| format!("--{name} precisa de um valor"))?,
            };
            p.vals.insert(name.to_string(), v);
        } else {
            return Err(format!("opcao desconhecida: --{name} (veja --help)"));
        }
    }
    Ok(p)
}

fn default_cfg() -> PathBuf {
    if std::path::Path::new("/home/ark/.config").is_dir() {
        return PathBuf::from("/home/ark/.config/led.cfg");
    }
    match env::var_os("HOME") {
        Some(h) => PathBuf::from(h).join(".config/led.cfg"),
        None => PathBuf::from("led.cfg"),
    }
}

fn io_err(e: io::Error) -> String {
    e.to_string()
}

fn parse_level(s: &str) -> Result<u8, String> {
    match s.trim().parse::<u8>() {
        Ok(n) if (BRI_MIN..=BRI_MAX).contains(&n) => Ok(n),
        _ => Err(format!(
            "brilho invalido: {s:?} (use {BRI_MIN}..{BRI_MAX}; para bytes fora disso use --bri-raw)"
        )),
    }
}

fn parse_secs(s: &str) -> Result<Duration, String> {
    match s.trim().parse::<f32>() {
        Ok(v) if v.is_finite() && (0.0..=3600.0).contains(&v) => Ok(Duration::from_secs_f32(v)),
        _ => Err(format!("tempo invalido: {s:?} (segundos, 0..3600)")),
    }
}

// ---------------------------------------------------------------------------
// Contexto e brilho
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    p: &'a Parsed,
    cfg: &'a Config,
    cfg_path: &'a PathBuf,
    leds: u32,
}

#[derive(Clone, Copy)]
enum Bri {
    Level(u8),
    Raw(u8),
}

impl Bri {
    fn byte(self) -> u8 {
        match self {
            Bri::Level(l) => bri_byte(l),
            Bri::Raw(b) => b,
        }
    }
    fn describe(self) -> String {
        match self {
            Bri::Level(l) => format!("{l} de {BRI_MAX} (byte {})", bri_byte(l)),
            Bri::Raw(b) => format!("byte cru {b} (0x{b:02X})"),
        }
    }
}

impl Ctx<'_> {
    /// --bri-raw > --bri > BRI do led.cfg.
    fn bri(&self) -> Result<Bri, String> {
        if let Some(v) = self.p.val("bri-raw") {
            return Ok(Bri::Raw(parse_byte(v)?));
        }
        match self.p.val("bri") {
            Some(v) => Ok(Bri::Level(parse_level(v)?)),
            None => Ok(Bri::Level(self.cfg.bri.clamp(BRI_MIN, BRI_MAX))),
        }
    }

    /// --mode ou MODE do led.cfg.
    fn base_mode(&self) -> Result<u8, String> {
        match self.p.val("mode") {
            Some(v) => parse_byte(v),
            None => Ok(self.cfg.mode),
        }
    }

    fn byte_opt(&self, k: &str) -> Result<Option<u8>, String> {
        self.p.val(k).map(parse_byte).transpose()
    }
}

fn describe_mode(b: u8) -> String {
    if b == MODE_OFF {
        return "OFF (apaga)".into();
    }
    if let Some(e) = SOLID.iter().find(|e| e.mode == b) {
        return format!("conhecido: {} fixo", e.label);
    }
    if let Some(e) = BREATHING.iter().find(|e| e.mode == b) {
        let tag = if e.verified {
            ""
        } else {
            " (deduzido, nao confirmado)"
        };
        return format!("conhecido: {} respirando{tag}", e.label);
    }
    "DESCONHECIDO - observe os LEDs".into()
}

fn describe_bri_byte(b: u8) -> String {
    match b {
        41..=49 => format!("conhecido: brilho {} de {BRI_MAX}", 50 - b),
        32..=40 => "regiao do init (0x20..) - desconhecido, observe".into(),
        _ => "fora da faixa conhecida (41..49) - observe".into(),
    }
}

// ---------------------------------------------------------------------------
// Sequencias
// ---------------------------------------------------------------------------

fn power_on(s: &mut Sess) {
    if let Err(e) = s.power(true) {
        eprintln!(
            "aviso: nao consegui ligar o trilho vcc-led ({e}); precisa de root e debugfs montado"
        );
    }
}

/// Trilho ligado + handshake: o mesmo comeco de toda troca do ledctl.sh.
fn start(s: &mut Sess) -> Result<(), String> {
    power_on(s);
    s.init().map_err(io_err)
}

fn apply(s: &mut Sess, c: &Ctx, mode: u8, bri: Bri) -> Result<(), String> {
    start(s)?;
    s.mode(mode).map_err(io_err)?;
    s.send(bri.byte()).map_err(io_err)?;
    s.close();
    println!("modo    {mode} (0x{mode:02X}) - {}", describe_mode(mode));
    println!("brilho  {}", bri.describe());
    if c.p.flag("save") {
        match bri {
            Bri::Level(level) => {
                let cfg = Config {
                    enabled: true,
                    mode,
                    bri: level,
                    ledcount: c.leds,
                };
                cfg.save(c.cfg_path).map_err(io_err)?;
                println!("salvo em {}", c.cfg_path.display());
            }
            Bri::Raw(_) => println!("(--save ignorado: brilho cru nao cabe no led.cfg)"),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Comandos
// ---------------------------------------------------------------------------

fn cmd_color(s: &mut Sess, c: &Ctx, args: &[String]) -> Result<(), String> {
    let name = args
        .first()
        .ok_or("color precisa de um nome: red green blue cyan yellow magenta rainbow scroll")?
        .to_lowercase();
    let (base_name, force_breath) = if name == "cycle" {
        ("scroll", true)
    } else {
        (name.as_str(), false)
    };
    let base = SOLID
        .iter()
        .find(|e| e.label.eq_ignore_ascii_case(base_name))
        .ok_or_else(|| format!("cor desconhecida: {name}"))?
        .mode;
    let mode = if c.p.flag("breath") || force_breath {
        base + 16
    } else {
        base
    };
    apply(s, c, mode, c.bri()?)
}

fn cmd_mode(s: &mut Sess, c: &Ctx, args: &[String]) -> Result<(), String> {
    let mode = parse_byte(args.first().ok_or("mode precisa de um byte (0..255)")?)?;
    apply(s, c, mode, c.bri()?)
}

fn cmd_bri(s: &mut Sess, c: &Ctx, args: &[String]) -> Result<(), String> {
    let level = parse_level(args.first().ok_or("bri precisa de um nivel 1..9")?)?;
    apply(s, c, c.base_mode()?, Bri::Level(level))
}

fn cmd_sweep(s: &mut Sess, c: &Ctx) -> Result<(), String> {
    let from = match c.p.val("from") {
        Some(v) => parse_level(v)?,
        None => BRI_MIN,
    };
    let to = match c.p.val("to") {
        Some(v) => parse_level(v)?,
        None => BRI_MAX,
    };
    if from > to {
        return Err("--from nao pode ser maior que --to".into());
    }
    let delay = parse_secs(c.p.val("delay").unwrap_or("1.5"))?;
    let mode = c.base_mode()?;
    start(s)?;
    s.mode(mode).map_err(io_err)?;
    println!(
        "sweep de brilho {from}..{to} no modo {mode} ({})",
        describe_mode(mode)
    );
    for b in from..=to {
        s.send(bri_byte(b)).map_err(io_err)?;
        println!("  brilho {b} (byte {})", bri_byte(b));
        thread::sleep(delay);
    }
    let back = c.cfg.bri.clamp(BRI_MIN, BRI_MAX);
    s.send(bri_byte(back)).map_err(io_err)?;
    s.close();
    println!("brilho restaurado para {back} (led.cfg)");
    Ok(())
}

fn cmd_raw(s: &mut Sess, c: &Ctx, args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("raw precisa de ao menos um byte (ex.: raw --init 3 44)".into());
    }
    let bytes = args
        .iter()
        .map(|a| parse_byte(a))
        .collect::<Result<Vec<u8>, String>>()?;
    power_on(s);
    if c.p.flag("init") {
        s.init().map_err(io_err)?;
        println!("init enviado");
    }
    for b in bytes {
        s.send(b).map_err(io_err)?;
        println!("byte {b} (0x{b:02X}) enviado");
    }
    s.close();
    Ok(())
}

fn cmd_off(s: &mut Sess) -> Result<(), String> {
    let r = s.send(MODE_OFF).map_err(io_err);
    s.close();
    if let Err(e) = s.power(false) {
        eprintln!("aviso: nao consegui cortar o trilho vcc-led ({e})");
    }
    r?;
    println!("LEDs apagados (byte {MODE_OFF}) e trilho desligado");
    Ok(())
}

fn cmd_status(c: &Ctx) {
    println!("led.cfg      {}", c.cfg_path.display());
    println!("ENABLED      {}", c.cfg.enabled as u8);
    println!(
        "MODE         {} - {}",
        c.cfg.mode,
        describe_mode(c.cfg.mode)
    );
    println!(
        "BRI          {} (byte {})",
        c.cfg.bri,
        bri_byte(c.cfg.bri.clamp(BRI_MIN, BRI_MAX))
    );
    println!("LEDCOUNT     {}", c.cfg.ledcount);
    match init_byte(c.leds) {
        Some(b) => println!("byte de init {b} (0x{b:02X}) para {} LEDs", c.leds),
        None => println!("byte de init nenhum ({} LEDs < 4)", c.leds),
    }
}

fn cmd_scan(s: &mut Sess, c: &Ctx) -> Result<(), String> {
    let kind = c.p.val("kind").unwrap_or("mode");
    let (def_from, def_to) = match kind {
        "mode" => (1u8, 31u8),
        "bri" => (36u8, 60u8),
        other => return Err(format!("--kind invalido: {other} (use mode ou bri)")),
    };
    let from = c.byte_opt("from")?.unwrap_or(def_from);
    let to = c.byte_opt("to")?.unwrap_or(def_to);
    if from > to {
        return Err("--from nao pode ser maior que --to".into());
    }
    let auto = c.p.val("auto").map(parse_secs).transpose()?;
    let bri = c.bri()?;
    let base_mode = c.base_mode()?;
    let resend = c.p.flag("resend-bri");

    start(s)?;
    if kind == "bri" {
        s.mode(base_mode).map_err(io_err)?;
        println!(
            "modo base {base_mode} ({}); os bytes da varredura vao na posicao do BRILHO",
            describe_mode(base_mode)
        );
    } else {
        println!(
            "brilho fixo {}; os bytes da varredura vao na posicao do MODO",
            bri.describe()
        );
    }
    println!("faixa {from}..{to}\n");

    let mut notes: Vec<(u8, String)> = Vec::new();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let mut i = from as i32;
    let mut first = true;
    while i <= to as i32 {
        let b = i as u8;
        if kind == "mode" {
            s.mode(b).map_err(io_err)?;
            if first || resend {
                s.send(bri.byte()).map_err(io_err)?;
            }
            println!("[{b:3} 0x{b:02X}] modo: {}", describe_mode(b));
        } else {
            s.send(b).map_err(io_err)?;
            println!("[{b:3} 0x{b:02X}] brilho: {}", describe_bri_byte(b));
        }
        first = false;

        if let Some(d) = auto {
            thread::sleep(d);
            i += 1;
            continue;
        }
        print!("  Enter=proximo  b=voltar  r=reenviar  g N=ir  q=sair  ou digite uma anotacao > ");
        let _ = io::stdout().flush();
        let Some(line) = lines.next() else { break };
        let line = line.map_err(io_err)?;
        let t = line.trim();
        match t {
            "" | "n" => i += 1,
            "b" => i = (i - 1).max(from as i32),
            "r" => {}
            "q" => break,
            _ => {
                if let Some(n) = t.strip_prefix("g ") {
                    match parse_byte(n) {
                        Ok(v) => i = v as i32,
                        Err(e) => println!("  {e}"),
                    }
                } else {
                    notes.push((b, t.to_string()));
                    i += 1;
                }
            }
        }
    }
    s.close();

    println!();
    if notes.is_empty() {
        println!("Nenhuma anotacao.");
    } else {
        println!("Anotacoes:");
        for (b, n) in &notes {
            println!("  {b:3} (0x{b:02X})  {n}");
        }
        if let Some(path) = c.p.val("out") {
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| format!("nao consegui abrir {path}: {e}"))?;
            for (b, n) in &notes {
                writeln!(f, "{kind}\t{b}\t0x{b:02X}\t{n}").map_err(io_err)?;
            }
            println!("anotacoes gravadas em {path}");
        }
    }
    println!("Os LEDs ficam no ultimo estado enviado. Para voltar: led-rgbctl color red  |  led-rgbctl off");
    Ok(())
}

// ---------------------------------------------------------------------------

fn run() -> Result<(), String> {
    let p = parse_args()?;
    if p.flag("help") || p.pos.is_empty() {
        print!("{HELP}");
        return Ok(());
    }
    let cfg_path = p.val("cfg").map(PathBuf::from).unwrap_or_else(default_cfg);
    let cfg = Config::load(&cfg_path);
    let repeat = match p.val("repeat") {
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| format!("--repeat invalido: {v:?}"))?,
        None => 6,
    };
    let leds = match p.val("leds") {
        Some(v) => v
            .parse::<u32>()
            .map_err(|_| format!("--leds invalido: {v:?}"))?,
        None => cfg.ledcount,
    };
    let opt = Options {
        repeat,
        always_init: false,
        ledcount: leds,
        timing: Timing::default(),
    };
    // `--dev` explicito e decisao de quem esta explorando o hardware.
    if !p.flag("dry-run") && p.val("dev").is_none() && p.pos[0] != "status" {
        let d = devices::resolve(None)?;
        if !d.supports(Feature::RgbLed) {
            return Err(format!("device incompatible: {}", d.label()));
        }
    }
    let transport: Box<dyn Transport> = if p.flag("dry-run") {
        Box::new(DryRun::verbose())
    } else {
        Box::new(UartTransport::new(
            p.val("dev").unwrap_or(DEFAULT_DEV),
            p.val("pwr").unwrap_or(DEFAULT_PWR),
        ))
    };
    let mut s = Session::new(transport, opt);
    let c = Ctx {
        p: &p,
        cfg: &cfg,
        cfg_path: &cfg_path,
        leds,
    };
    let args = &p.pos[1..];
    match p.pos[0].as_str() {
        "color" => cmd_color(&mut s, &c, args),
        "mode" => cmd_mode(&mut s, &c, args),
        "bri" => cmd_bri(&mut s, &c, args),
        "sweep" => cmd_sweep(&mut s, &c),
        "scan" => cmd_scan(&mut s, &c),
        "raw" => cmd_raw(&mut s, &c, args),
        "off" => cmd_off(&mut s),
        "status" => {
            cmd_status(&c);
            Ok(())
        }
        other => Err(format!("comando desconhecido: {other} (veja --help)")),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("erro: {e}");
        process::exit(1);
    }
}
