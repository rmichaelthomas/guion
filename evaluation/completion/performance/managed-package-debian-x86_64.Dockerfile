FROM rust:1.97-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates cmake curl python3 \
    && rm -rf /var/lib/apt/lists/*

RUN curl -LsSf https://astral.sh/uv/0.13.4/install.sh | sh \
    && install -m 0755 /root/.local/bin/uv /usr/local/bin/uv \
    && uv --version \
    && python3 --version \
    && cmake --version

RUN mkdir -p /opt/guion-managed-warmed/cache \
    && UV_CACHE_DIR=/tmp/uv-bootstrap-cache uv python install 3.14.8 --install-dir /tmp/veritas-linux-python \
    && test -x /tmp/veritas-linux-python/cpython-3.14.8-linux-x86_64-gnu/bin/python3.14 \
    && /tmp/veritas-linux-python/cpython-3.14.8-linux-x86_64-gnu/bin/python3.14 --version \
    && mkdir -p /tmp/guion-python-bridge \
    && printf '%s\n' \
        '[project]' \
        'name = "guion-python-bridge"' \
        'version = "0.0.0"' \
        'requires-python = "==3.14.8"' \
        'dependencies = ["orjson==3.13.0", "numpy==2.5.4"]' \
        > /tmp/guion-python-bridge/pyproject.toml \
    && UV_CACHE_DIR=/opt/guion-managed-warmed/cache UV_PYTHON_INSTALL_DIR=/tmp/veritas-linux-python \
        uv lock --project /tmp/guion-python-bridge --python 3.14.8 --managed-python --upgrade \
    && UV_CACHE_DIR=/opt/guion-managed-warmed/cache UV_PYTHON_INSTALL_DIR=/tmp/veritas-linux-python \
        uv sync --project /tmp/guion-python-bridge --locked --python 3.14.8 --managed-python \
    && rm -rf /tmp/guion-python-bridge/.venv /tmp/guion-python-bridge/uv.lock \
    && UV_CACHE_DIR=/opt/guion-managed-warmed/cache UV_PYTHON_INSTALL_DIR=/tmp/veritas-linux-python UV_OFFLINE=1 \
        uv lock --project /tmp/guion-python-bridge --python 3.14.8 --managed-python --upgrade \
    && UV_CACHE_DIR=/opt/guion-managed-warmed/cache UV_PYTHON_INSTALL_DIR=/tmp/veritas-linux-python UV_OFFLINE=1 \
        uv sync --project /tmp/guion-python-bridge --locked --python 3.14.8 --managed-python \
    && rm -rf /tmp/guion-python-bridge /tmp/uv-bootstrap-cache \
    && chown -R 501:20 /opt/guion-managed-warmed/cache
