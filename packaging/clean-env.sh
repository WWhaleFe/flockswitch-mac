# Sourced before a local release build: keeps this computer's paths (user name, disk
# and folder names) out of the binary. Rust and the C compiler both get the mapping.
_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$_root=. --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=cargo --remap-path-prefix=$HOME/.rustup=rustup --remap-path-prefix=$HOME=~"
export CFLAGS="${CFLAGS:-} -ffile-prefix-map=$_root=. -ffile-prefix-map=$HOME=~"
unset _root
