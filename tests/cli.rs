//! Testes do `led-rgbctl` rodando o binario de verdade em --dry-run e conferindo
//! os bytes que iriam para a MCU (eventos impressos em stderr).

use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

struct Out {
    ok: bool,
    stdout: String,
    stderr: String,
    /// Bytes enviados, na ordem (`Byte(n)` no stderr).
    bytes: Vec<u8>,
}

fn ctl(args: &[&str], stdin: &str) -> Out {
    // Padroes primeiro: se o teste repetir uma opcao (ex.: --cfg), a ultima vence.
    let mut child = Command::new(env!("CARGO_BIN_EXE_led-rgbctl"))
        .args([
            "--dry-run",
            "--repeat",
            "1",
            "--cfg",
            "/nonexistent/dir/led.cfg",
        ])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("nao consegui rodar led-rgbctl");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    let bytes = stderr
        .lines()
        .filter_map(|l| l.split("Byte(").nth(1))
        .filter_map(|r| r.trim_end_matches(')').parse().ok())
        .collect();
    Out {
        ok: o.status.success(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr,
        bytes,
    }
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("led_rgbctl-{name}-{}", std::process::id()))
}

#[test]
fn color_envia_init_modo_brilho() {
    let o = ctl(&["color", "red", "--bri", "9"], "");
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(o.bytes, [0x22, 3, 41]);
    assert!(o.stderr.contains("Power(true)"));
}

#[test]
fn color_respirando_soma_16_e_cycle_e_scroll_respirando() {
    assert_eq!(
        ctl(&["color", "rainbow", "--breath", "--bri", "1"], "").bytes,
        [0x22, 23, 49]
    );
    assert_eq!(ctl(&["color", "cycle"], "").bytes, [0x22, 24, 45]);
    assert_eq!(ctl(&["color", "GREEN"], "").bytes, [0x22, 1, 45]);
}

#[test]
fn mode_aceita_qualquer_byte_e_hex() {
    assert_eq!(ctl(&["mode", "12"], "").bytes, [0x22, 12, 45]);
    assert_eq!(
        ctl(&["mode", "0x1F", "--bri", "3"], "").bytes,
        [0x22, 31, 47]
    );
}

#[test]
fn bri_raw_substitui_o_byte_de_brilho() {
    assert_eq!(
        ctl(&["mode", "3", "--bri-raw", "50"], "").bytes,
        [0x22, 3, 50]
    );
    assert_eq!(
        ctl(&["mode", "3", "--bri-raw", "0x2C"], "").bytes,
        [0x22, 3, 44]
    );
}

#[test]
fn bri_usa_o_modo_do_cfg_ou_do_flag() {
    let cfg = tmp("bri.cfg");
    std::fs::write(&cfg, "ENABLED=1\nMODE=19\nBRI=5\nLEDCOUNT=12\n").unwrap();
    let c = cfg.to_str().unwrap();
    assert_eq!(ctl(&["bri", "7", "--cfg", c], "").bytes, [0x22, 19, 43]);
    assert_eq!(
        ctl(&["bri", "7", "--mode", "2", "--cfg", c], "").bytes,
        [0x22, 2, 43]
    );
    let _ = std::fs::remove_file(cfg);
}

#[test]
fn leds_muda_o_byte_de_init() {
    assert_eq!(ctl(&["mode", "3", "--leds", "8"], "").bytes[0], 0x21);
    assert_eq!(ctl(&["mode", "3", "--leds", "4"], "").bytes[0], 0x20);
    // < 4 LEDs: sem handshake
    assert_eq!(ctl(&["mode", "3", "--leds", "3"], "").bytes, [3, 45]);
}

#[test]
fn raw_nao_faz_init_a_menos_que_peca() {
    assert_eq!(ctl(&["raw", "7", "9", "0x2C"], "").bytes, [7, 9, 44]);
    assert_eq!(ctl(&["raw", "--init", "3", "44"], "").bytes, [0x22, 3, 44]);
    let o = ctl(&["raw"], "");
    assert!(!o.ok);
    assert!(o.stderr.contains("raw precisa"));
}

#[test]
fn off_manda_9_e_corta_o_trilho() {
    let o = ctl(&["off"], "");
    assert!(o.ok);
    assert_eq!(o.bytes, [9]);
    assert!(o.stderr.contains("Power(false)"));
    assert!(!o.stderr.contains("Power(true)"));
}

#[test]
fn sweep_varre_o_brilho_e_restaura_o_do_cfg() {
    let o = ctl(
        &[
            "sweep", "--from", "1", "--to", "3", "--delay", "0", "--mode", "3",
        ],
        "",
    );
    assert!(o.ok, "{}", o.stderr);
    // init, modo, brilhos 1..3 (49,48,47) e o BRI padrao (5 => 45) de volta
    assert_eq!(o.bytes, [0x22, 3, 49, 48, 47, 45]);
}

#[test]
fn save_grava_o_led_cfg() {
    let cfg = tmp("save.cfg");
    let c = cfg.to_str().unwrap();
    let o = ctl(&["color", "cyan", "--bri", "6", "--save", "--cfg", c], "");
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(
        std::fs::read_to_string(&cfg).unwrap(),
        "ENABLED=1\nMODE=4\nBRI=6\nLEDCOUNT=12\n"
    );
    // brilho cru nao e gravado
    let cfg2 = tmp("save2.cfg");
    let c2 = cfg2.to_str().unwrap();
    let o = ctl(&["mode", "4", "--bri-raw", "50", "--save", "--cfg", c2], "");
    assert!(o.ok);
    assert!(!cfg2.exists());
    let _ = std::fs::remove_file(cfg);
}

#[test]
fn scan_de_modos_automatico() {
    let o = ctl(
        &[
            "scan", "--from", "1", "--to", "3", "--auto", "0", "--bri", "5",
        ],
        "",
    );
    assert!(o.ok, "{}", o.stderr);
    // init, modo 1 + brilho (so no primeiro), depois so os modos 2 e 3
    assert_eq!(o.bytes, [0x22, 1, 45, 2, 3]);
}

#[test]
fn scan_resend_bri_reenvia_o_brilho_a_cada_modo() {
    let o = ctl(
        &[
            "scan",
            "--from",
            "1",
            "--to",
            "2",
            "--auto",
            "0",
            "--resend-bri",
        ],
        "",
    );
    assert_eq!(o.bytes, [0x22, 1, 45, 2, 45]);
}

#[test]
fn scan_de_brilho_envia_os_bytes_na_posicao_do_brilho() {
    let o = ctl(
        &[
            "scan", "--kind", "bri", "--from", "41", "--to", "43", "--auto", "0", "--mode", "3",
        ],
        "",
    );
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(o.bytes, [0x22, 3, 41, 42, 43]);
    assert!(o.stdout.contains("brilho 9 de 9")); // byte 41
    assert!(o.stdout.contains("fora da faixa conhecida") || o.stdout.contains("conhecido"));
}

#[test]
fn scan_interativo_anota_volta_repete_e_sai() {
    let out = tmp("notes.txt");
    let _ = std::fs::remove_file(&out);
    // 1: anota e avanca | 2: Enter | 3: b (volta p/ 2) | 2: r (repete) | 2: q
    let stdin = "azul piscando\n\nb\nr\nq\n";
    let o = ctl(
        &[
            "scan",
            "--from",
            "1",
            "--to",
            "5",
            "--out",
            out.to_str().unwrap(),
        ],
        stdin,
    );
    assert!(o.ok, "{}", o.stderr);
    // modos enviados (descartando init 34 e o brilho 45 do primeiro passo)
    assert_eq!(o.bytes, [0x22, 1, 45, 2, 3, 2, 2]);
    assert!(o.stdout.contains("Anotacoes:"));
    assert!(o.stdout.contains("azul piscando"));
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "mode\t1\t0x01\tazul piscando\n"
    );
    let _ = std::fs::remove_file(out);
}

#[test]
fn scan_g_pula_para_o_byte_pedido_e_ignora_g_invalido() {
    let o = ctl(&["scan", "--from", "1", "--to", "20"], "g 0x10\ng zz\nq\n");
    assert!(o.ok);
    // 1 (init+modo+brilho), depois pulou para 16, depois "g zz" repete 16
    assert_eq!(o.bytes, [0x22, 1, 45, 16, 16]);
    assert!(o.stdout.contains("byte invalido"));
}

#[test]
fn scan_termina_se_o_stdin_acabar() {
    let o = ctl(&["scan", "--from", "1", "--to", "31"], "");
    assert!(o.ok);
    assert_eq!(o.bytes, [0x22, 1, 45]);
}

#[test]
fn status_nao_toca_no_hardware() {
    let o = ctl(&["status"], "");
    assert!(o.ok);
    assert!(o.bytes.is_empty());
    assert!(!o.stderr.contains("Power"));
    assert!(o.stdout.contains("0x22"));
}

#[test]
fn entradas_invalidas_dao_erro_claro() {
    for (args, msg) in [
        (&["mode", "300"][..], "fora de 0..255"),
        (&["mode"][..], "mode precisa"),
        (&["bri", "0"][..], "brilho invalido"),
        (&["bri", "10"][..], "brilho invalido"),
        (&["color", "roxo"][..], "cor desconhecida"),
        (&["color"][..], "color precisa"),
        (&["scan", "--kind", "x"][..], "--kind invalido"),
        (&["scan", "--from", "9", "--to", "3"][..], "--from"),
        (&["sweep", "--delay", "-1"][..], "tempo invalido"),
        (&["--bogus"][..], "opcao desconhecida"),
        (&["voar"][..], "comando desconhecido"),
        (&["mode", "3", "--repeat", "x"][..], "--repeat invalido"),
    ] {
        let o = ctl(args, "");
        assert!(!o.ok, "{args:?} deveria falhar");
        assert!(o.stderr.contains(msg), "{args:?}: {}", o.stderr);
        // nenhum byte enviado quando a entrada e invalida
        assert!(o.bytes.is_empty(), "{args:?} enviou {:?}", o.bytes);
    }
}

#[test]
fn ajuda_lista_os_comandos() {
    let o = ctl(&["--help"], "");
    assert!(o.ok);
    for w in ["color", "scan", "raw", "sweep", "--bri-raw", "--dry-run"] {
        assert!(o.stdout.contains(w), "falta {w} na ajuda");
    }
}
