FROM ubuntu:24.04
LABEL Name=x3rust Version=0.0.1

# Install dependencies, this includes dependencies for benchmarking and testing.
RUN apt update && \
    apt dist-upgrade -y && \
    apt install -y build-essential curl git flac time bc llvm-20 sudo

# ── Non-root user ──────────────────────────────────────────────────────────────
ARG USERNAME=ubuntu
ARG USER_UID=1000
ARG USER_GID=${USER_UID}

RUN mkdir -p /workspace \
    && chown -R ${USERNAME}:${USERNAME} /workspace \
    && echo "${USERNAME} ALL=(ALL) NOPASSWD:ALL" >> /etc/sudoers.d/${USERNAME} \
    && chmod 0440 /etc/sudoers.d/${USERNAME}

USER ${USERNAME}

# ── Install Rust ───────────────────────────────────────────────────────────────

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/home/${USERNAME}/.cargo/bin:${PATH}"

# This repo is used for bechmarks
RUN git config --global --add safe.directory /workspaces/x3-rust
RUN git config --global --add safe.directory /workspaces/x3-rust/test/underwater-sound-samples

ENTRYPOINT ["sh"]
