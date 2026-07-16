#!/usr/bin/env zsh
# treeizsh developer module activation script
#
# Usage: ./scripts/dev-build.zsh [--release|--debug]
#
# This script is for local development only. It builds the Rust module, and
# starts another zsh shell with the new module loaded for interactive testing.

WORKSPACE_DIR="${0:A:h:h}"

() {
    emulate -L zsh

    local build_type="debug"
    if (( ${@[(Ie)--release]} )); then
        build_type="release"
    fi

    # Build/Rebuild native module if missing or stale
    local OS="$(uname -s)"
    local LIB_NAME
    if [[ "$OS" == "Darwin" ]]; then
        LIB_NAME="libtreeizsh.dylib"
    else
        LIB_NAME="libtreeizsh.so"
    fi
    local LIB_PATH="$WORKSPACE_DIR/target/${build_type}/${LIB_NAME}"

    rebuild_module() {
        print -P "%F{yellow}%BBuilding/rebuilding native module ($build_type) for treeizsh...%b%f"
        local active_zsh
        active_zsh="$(which zsh)"

        local cargo_args=(--manifest-path "$WORKSPACE_DIR/Cargo.toml" -p treeizsh-module --lib)
        if [[ "$build_type" == "release" ]]; then
            cargo_args+=(--release)
        fi

        ZSH_BINARY="$active_zsh" cargo build "${cargo_args[@]}"
        local build_status=$?

        # Sleep a bit to allow OS validation and filesystem sync of the new .dylib/so
        sleep 1

        return $build_status
    }

    local needs_build=0
    if [[ ! -f "$LIB_PATH" ]]; then
        needs_build=1
    elif command -v fd >/dev/null && command -v gstat >/dev/null; then
        local STALE_FILES
        STALE_FILES="$(fd --changed-after "@$(gstat -c %Y "$LIB_PATH")" . "$WORKSPACE_DIR")"
        if [[ -n "$STALE_FILES" ]]; then
            print -P "%F{yellow}%BNative module ($build_type) is stale. Modified files:%b%f"
            echo "$STALE_FILES"
            needs_build=1
        fi
    else
        # Fallback to building if fd or gstat are not available
        needs_build=1
    fi

    if (( needs_build )); then
        if ! rebuild_module; then
            print -P -u2 "%F{red}treeizsh: cargo build failed%f"
            return 1
        fi
        print
    else
        print -P "%F{green}‣ Native module ($build_type) is up-to-date. Skipping build.%f"
        print
    fi

    local temp_dir="$(mktemp -d)"
    print -P "%F{8}‣ Creating temporary zsh dotfile directory: ${temp_dir}%f"
    print
    cat <<EOF >"${temp_dir}/.zshrc"
TREEIZSH_MODULE_PATH="${WORKSPACE_DIR}/target/${build_type}" source "${WORKSPACE_DIR}/treeizsh-module/treeizsh.zsh"
EOF

    print -P "%B‣ Starting a subshell. Run 'exit' (Ctrl-D) to return.%b"
    print
    ZDOTDIR="${temp_dir}" zsh -i
    rm -rf "${temp_dir}/"
} "$@" || return $?
