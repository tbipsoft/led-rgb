//! Sessao de baixo nivel: uma porta aberta durante varios envios.
//!
//! Usada pela ferramenta de linha de comando (`led-rgbctl`) para testes e
//! exploracao: ao contrario do `Controller`, nao decide nada sozinha (nao faz init
//! automatico, nao valida modo nem brilho) -- so envia os bytes pedidos.

use std::{io, thread};

use crate::driver::{init_byte, Options, Transport};

/// Aceita decimal (`34`) ou hexadecimal (`0x22`), de 0 a 255.
pub fn parse_byte(s: &str) -> Result<u8, String> {
    let t = s.trim();
    let (digits, radix) = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(h) => (h, 16),
        None => (t, 10),
    };
    let n = u32::from_str_radix(digits, radix)
        .map_err(|_| format!("byte invalido: {s:?} (use 0..255 ou 0x00..0xFF)"))?;
    u8::try_from(n).map_err(|_| format!("byte fora de 0..255: {s:?}"))
}

pub struct Session<T: Transport> {
    t: T,
    opt: Options,
    open: bool,
}

impl<T: Transport> Session<T> {
    pub fn new(t: T, opt: Options) -> Self {
        Session {
            t,
            opt,
            open: false,
        }
    }

    /// Liga/desliga o trilho de alimentacao dos LEDs.
    pub fn power(&mut self, on: bool) -> io::Result<()> {
        self.t.power(on)
    }

    /// Envia `byte` `repeat` vezes (abre a UART na primeira chamada).
    pub fn send(&mut self, byte: u8) -> io::Result<()> {
        if !self.open {
            self.t.open()?;
            self.open = true;
        }
        for _ in 0..self.opt.repeat.max(1) {
            self.t.send_byte(byte)?;
            thread::sleep(self.opt.timing.gap);
        }
        Ok(())
    }

    /// Handshake `init <ledcount>` + pausa. Sem efeito se `ledcount < 4`.
    pub fn init(&mut self) -> io::Result<()> {
        if let Some(b) = init_byte(self.opt.ledcount) {
            self.send(b)?;
            thread::sleep(self.opt.timing.after_init);
        }
        Ok(())
    }

    /// Byte de modo + pausa (como o ledctl.sh antes de mandar o brilho).
    pub fn mode(&mut self, mode: u8) -> io::Result<()> {
        self.send(mode)?;
        thread::sleep(self.opt.timing.after_mode);
        Ok(())
    }

    /// Fecha a UART (os LEDs mantem o ultimo estado).
    pub fn close(&mut self) {
        if self.open {
            self.t.close();
            self.open = false;
        }
    }
}

impl<T: Transport> Drop for Session<T> {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{DryRun, Event::*, Timing};

    #[test]
    fn parse_byte_decimal_e_hex() {
        assert_eq!(parse_byte("0"), Ok(0));
        assert_eq!(parse_byte("34"), Ok(34));
        assert_eq!(parse_byte(" 255 "), Ok(255));
        assert_eq!(parse_byte("0x22"), Ok(34));
        assert_eq!(parse_byte("0XfF"), Ok(255));
        assert!(parse_byte("256").is_err());
        assert!(parse_byte("0x100").is_err());
        assert!(parse_byte("-1").is_err());
        assert!(parse_byte("").is_err());
        assert!(parse_byte("0x").is_err());
        assert!(parse_byte("abc").is_err());
    }

    #[test]
    fn abre_uma_vez_e_repete_cada_byte() {
        let (t, log) = DryRun::recorder();
        let opt = Options {
            repeat: 2,
            timing: Timing::none(),
            ..Options::default()
        };
        let mut s = Session::new(t, opt);
        s.init().unwrap();
        s.mode(7).unwrap();
        s.send(45).unwrap();
        s.close();
        assert_eq!(
            *log.lock().unwrap(),
            vec![
                Open,
                Byte(0x22),
                Byte(0x22),
                Byte(7),
                Byte(7),
                Byte(45),
                Byte(45),
                Close
            ]
        );
    }

    #[test]
    fn fecha_ao_sair_de_escopo() {
        let (t, log) = DryRun::recorder();
        {
            let mut s = Session::new(t, Options::default());
            s.send(1).unwrap();
        }
        assert_eq!(log.lock().unwrap().last(), Some(&Close));
    }
}
