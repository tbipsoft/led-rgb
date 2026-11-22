//! Valida o `UartTransport` real contra um pseudo-terminal (no lugar da /dev/ttyS1):
//! confere a configuracao 9600 8N1 raw e os bytes que chegam "na MCU".

use std::{
    ffi::CStr,
    fs::File,
    io::Read,
    os::unix::io::{AsRawFd, FromRawFd},
    path::PathBuf,
    time::Duration,
};

use led_rgb::{Controller, Desired, Options, Timing, UartTransport};

fn openpty() -> (File, PathBuf, File) {
    let (mut m, mut s) = (0, 0);
    let mut name = [0 as libc::c_char; 128];
    // SAFETY: ponteiros validos para openpty; os fds retornados passam a ser donos dos File.
    let rc = unsafe {
        libc::openpty(
            &mut m,
            &mut s,
            name.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(rc, 0, "openpty falhou");
    let path = unsafe { CStr::from_ptr(name.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: fds recem-criados e exclusivos.
    unsafe {
        (
            File::from_raw_fd(m),
            PathBuf::from(path),
            File::from_raw_fd(s),
        )
    }
}

#[test]
fn bytes_chegam_na_mcu_com_9600_8n1() {
    let (mut master, slave_path, slave_keepalive) = openpty();

    // Rail de mentira: um arquivo comum.
    let pwr = std::env::temp_dir().join(format!("led_rgb-pwr-{}", std::process::id()));
    std::fs::write(&pwr, "0").unwrap();

    let opt = Options {
        repeat: 3,
        timing: Timing::none(),
        ..Options::default()
    };
    let mut c = Controller::new(UartTransport::new(&slave_path, &pwr), opt);
    c.apply(Desired {
        enabled: true,
        mode: 19,
        bri: 5,
    })
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(&pwr).unwrap(),
        "1",
        "trilho deveria ser ligado"
    );

    // Le o que a "MCU" recebeu: init x3, modo x3, brilho x3.
    // (mesmo apos o close do escravo, os dados ja escritos continuam legiveis no master)
    let mut got = Vec::new();
    let mut buf = [0u8; 64];
    // O master e bloqueante; ja ha dados prontos, entao um unico read basta,
    // mas tentamos algumas vezes ate completar 9 bytes.
    set_nonblock(master.as_raw_fd());
    for _ in 0..100 {
        match master.read(&mut buf) {
            Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
        if got.len() >= 9 {
            break;
        }
    }
    assert_eq!(got, [0x22, 0x22, 0x22, 19, 19, 19, 45, 45, 45]);

    // Configuracao do escravo: 9600, 8 bits, sem paridade, 1 stop, CLOCAL.
    // Relemos pela fd do escravo mantida aberta pelo teste.
    let t = termios_of(slave_keepalive.as_raw_fd());
    assert_eq!(unsafe { libc::cfgetospeed(&t) }, libc::B9600);
    assert_eq!(t.c_cflag & libc::CSIZE, libc::CS8);
    assert_eq!(t.c_cflag & libc::PARENB, 0);
    assert_eq!(t.c_cflag & libc::CSTOPB, 0);
    assert_ne!(t.c_cflag & libc::CLOCAL, 0);
    assert_eq!(t.c_lflag & libc::ECHO, 0, "tem que estar em modo raw");
    assert_eq!(t.c_lflag & libc::ICANON, 0, "tem que estar em modo raw");

    // Desligar: modo 9 e trilho a 0.
    c.apply(Desired {
        enabled: false,
        mode: 3,
        bri: 5,
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(&pwr).unwrap(), "0");
    let mut off = Vec::new();
    for _ in 0..100 {
        match master.read(&mut buf) {
            Ok(n) if n > 0 => off.extend_from_slice(&buf[..n]),
            _ => std::thread::sleep(Duration::from_millis(5)),
        }
        if off.len() >= 3 {
            break;
        }
    }
    assert_eq!(off, [9, 9, 9]);

    let _ = std::fs::remove_file(&pwr);
}

fn set_nonblock(fd: i32) {
    // SAFETY: fcntl em fd valido.
    unsafe {
        let fl = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
    }
}

fn termios_of(fd: i32) -> libc::termios {
    // SAFETY: tcgetattr preenche a struct para um fd valido.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(fd, &mut t), 0);
        t
    }
}
