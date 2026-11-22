#!/bin/bash
#
# LED RGB -- launcher do menu "Tools" do dArkOS para o app led-rgb
# (cores dos LEDs dos analogicos do RG52 Mini / RG43 Pro, RK3562).
#
# Segue o mesmo padrao do "LED Settings.sh" do dArkOS: roda como o usuario ark,
# usa sudo para o que precisa de root (a UART /dev/ttyS1 e o debugfs do trilho
# vcc-led sao acessados pelo proprio app, que e executado com sudo) e usa a tty1.
#
# Onde colocar: este .sh na mesma pasta dos outros scripts do menu Tools (onde
# esta o "LED Settings.sh") e o binario `led-rgb` numa destas opcoes:
#   <pasta deste script>/led-rgb/led-rgb      (pasta "led-rgb" ao lado do .sh)
#   <pasta deste script>/led-rgb               (binario ao lado do .sh)
#   /opt/led-rgb/led-rgb
#
# Em aparelho sem esse LED o app mostra "DEVICE INCOMPATIBLE" e nao toca na serial.
#   DEVICE_PROFILE=r36s  forca o perfil (rg52mini, rg43, r36s, rgb10, a10mini, rk3566, generic)
#
# NAO use junto com o "LED Settings.sh": os dois escrevem na mesma serial.

APP=led-rgb
CFG=/home/ark/.config/led.cfg
LOG=/tmp/led-rgb.log
RUN=/tmp/led-rgb.run
TTY=${TTY:-/dev/tty1}

# Entrada de controle:
#   0 = o proprio app le o gamepad pelo SDL (padrao)
#   1 = gptokeyb converte o gamepad em teclado, como no LED Settings.sh
#       (se escolher 1, confira se /opt/inttools/keys.gptk mapeia D-pad, A, B, L1, R1)
USE_GPTOKEYB=${USE_GPTOKEYB:-0}

HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"

say() { # mensagem na tela do aparelho, por alguns segundos
  printf '\033c%s\n' "$1" > "$TTY" 2>/dev/null
  sleep "${2:-5}"
}

find_bin() {
  local c
  for c in "$HERE/$APP/$APP" "$HERE/$APP" "/opt/$APP/$APP"; do
    [ -f "$c" ] && { echo "$c"; return 0; }
  done
  return 1
}

cleanup() {
  [ "$USE_GPTOKEYB" = "1" ] && pgrep -f gptokeyb | sudo xargs -r kill -9
  rm -f "$RUN"
  printf '\033c' > "$TTY" 2>/dev/null
}

sudo chmod 666 "$TTY" 2>/dev/null
printf '\033c\e[?25l' > "$TTY" 2>/dev/null   # limpa a tela e esconde o cursor

BIN=$(find_bin) || {
  say "led-rgb nao encontrado.

Coloque o binario em uma destas opcoes:
  $HERE/$APP/$APP
  $HERE/$APP
  /opt/$APP/$APP" 8
  cleanup
  exit 1
}

# Copia para /tmp e marca como executavel: funciona mesmo que o binario esteja numa
# particao FAT (SD de roms), onde o bit de execucao nao existe ou a montagem e noexec.
if ! cp -f "$BIN" "$RUN" || ! chmod +x "$RUN"; then
  say "Nao consegui preparar $RUN (disco cheio ou /tmp somente leitura?)" 6
  cleanup
  exit 1
fi

export SDL_GAMECONTROLLERCONFIG_FILE="/opt/inttools/gamecontrollerdb.txt"
export XDG_RUNTIME_DIR="/run/user/$UID/"

ARGS=(--fullscreen --cfg "$CFG")
[ -n "$DEVICE_PROFILE" ] && ARGS+=(--device "$DEVICE_PROFILE")
if [ "$USE_GPTOKEYB" = "1" ]; then
  sudo chmod 666 /dev/uinput
  pgrep -f gptokeyb | sudo xargs -r kill -9
  /opt/inttools/gptokeyb -1 "$(basename "$RUN")" -c "/opt/inttools/keys.gptk" > /dev/null 2>&1 &
  ARGS+=(--no-gamepad)   # evita contar cada botao duas vezes (teclado + gamepad)
fi

# `env` repassa as variaveis ao app mesmo que o sudoers nao permita `sudo -E`.
# SDL_VIDEODRIVER so e repassada se voce a definiu (ex.: SDL_VIDEODRIVER=kmsdrm).
sudo env \
  SDL_GAMECONTROLLERCONFIG_FILE="$SDL_GAMECONTROLLERCONFIG_FILE" \
  XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" \
  ${SDL_VIDEODRIVER:+SDL_VIDEODRIVER="$SDL_VIDEODRIVER"} \
  "$RUN" "${ARGS[@]}" > "$LOG" 2>&1
STATUS=$?

if [ "$STATUS" -ne 0 ]; then
  say "led-rgb terminou com erro ($STATUS).

$(tail -n 12 "$LOG" 2>/dev/null)

Log completo: $LOG" 10
fi

cleanup
exit "$STATUS"
