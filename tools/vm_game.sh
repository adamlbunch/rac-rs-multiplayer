#!/bin/sh
# vm_game.sh - one game on a Linux machine (the WSL VM standing in for a second PC, or any Linux PC with setup.sh run), walked
# into play on Veldin with no hands on the pad, as tools/mp_play.ps1 does on Windows: the title pilot presses Start and New
# Game, the key taps skip the two movies and the opening scene. It runs on a virtual screen of its own (Xvfb), muted, so
# nothing shows on the desktop; its run log is PLAYER/log.txt.
#
#   tools/vm_game.sh --mp join:172.24.240.1:2410 --name Joiner --player ~/racvm/join --fields 5000 [--pad "..."] [--travel 1:1800]
#                    [--iso ~/racvm/usa.iso]
#
# Waits for the game to end (--fields, or killed) and prints its exit code.
unset LD_PRELOAD
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd) || exit 1
mp= name=Ratchet player= pad= fields=0 travel= iso=
while [ $# -gt 0 ]; do
	case "$1" in
		--mp) mp=$2; shift 2 ;;
		--name) name=$2; shift 2 ;;
		--player) player=$2; shift 2 ;;
		--pad) pad=$2; shift 2 ;;
		--fields) fields=$2; shift 2 ;;
		--travel) travel=$2; shift 2 ;;
		--iso) iso=$2; shift 2 ;;
		*) echo "unknown argument: $1" >&2; exit 1 ;;
	esac
done
exe="$root/build/release/rac-rs"
[ -x "$exe" ] || { echo "no game at $exe: run setup.sh first" >&2; exit 1; }
[ -n "$iso" ] || iso=$(sed -n 's/^iso=//p' "$root/generated/play.cfg" | head -n 1)
[ -n "$player" ] || player="${TMPDIR:-/tmp}/racmp/player-$(date +%H%M%S)"
mkdir -p "$player" || exit 1

# A virtual screen of its own: the first free display from :90.
n=90
while [ -e "/tmp/.X11-unix/X$n" ] || [ -e "/tmp/.X$n-lock" ]; do n=$((n + 1)); done
Xvfb ":$n" -screen 0 1280x720x24 -nolisten tcp >"$player/xvfb.txt" 2>&1 &
xvfb=$!
i=0
while [ ! -e "/tmp/.X11-unix/X$n" ] && [ $i -lt 50 ]; do sleep 0.1; i=$((i + 1)); done

export DISPLAY=":$n"
unset WAYLAND_DISPLAY
export ORACLE_ISO="$iso"
export ORACLE_STUBS="$root/oracle/stubs.txt"
ORACLE_HAND_SKIP=$(head -n 1 "$root/oracle/hand/hand_skip.txt" | tr -d '\r')
export ORACLE_HAND_SKIP
export RAC_PLAYER_DIR="$player"
export RAC_NO_UPDATE_CHECK=1
export RAC_TITLE_PILOT='title:30,start,cross,wait:120,circle,wait:90,cross,wait:120'
export RAC_RETAIL_KEYTAP='Escape@movie1+30;Escape@movie2+30;Escape@scene+30'
export RAC_TEST_WINDOW=hidden
export RAC_MUTE=1
export RAC_RUN_LOG=1
# A small picture: with no GPU in a VM the software Vulkan driver (llvmpipe) draws, and fewer pixels are a little faster.
export RAC_TEST_SIZE="${RAC_TEST_SIZE:-640x360}"
unset RAC_MP RAC_MP_NAME RAC_RETAIL_PAD RAC_RETAIL_SMOKE RAC_RETAIL_TRAVEL
if [ -n "$mp" ]; then export RAC_MP="$mp" RAC_MP_NAME="$name"; fi
if [ -n "$pad" ]; then export RAC_RETAIL_PAD="$pad"; fi
if [ -n "$travel" ]; then export RAC_RETAIL_TRAVEL="$travel"; fi
if [ "$fields" -gt 0 ]; then export RAC_RETAIL_SMOKE="$fields"; fi

cd "$root" || exit 1
"$exe" view "$iso" 0 --retail-cold --modern-cam >"$player/stdout.txt" 2>&1 &
game=$!
echo "pid $game, display :$n, player folder $player (log: log.txt)"
trap 'kill $game 2>/dev/null' INT TERM HUP
trap 'kill $xvfb 2>/dev/null' EXIT
wait $game
rc=$?
echo "exit $rc"
