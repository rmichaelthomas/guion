ARG PYTHON_BASE=python:3.14.8-slim-bookworm@sha256:44e000a07b48bef1517a2b14adc93c17af728ca98858b5e24c6e1b18de1b2ef2
ARG UV_VERSION=0.13.4

FROM ${PYTHON_BASE} AS release-extract
COPY veritas-linux-aarch64.tar.gz /tmp/veritas-linux-aarch64.tar.gz
RUN mkdir -p /tmp/release /opt/campaign-python \
    && tar -xzf /tmp/veritas-linux-aarch64.tar.gz -C /tmp/release \
    && cp -a /tmp/release/veritas-0.1.0-linux-aarch64/examples/python_interop_bundle/python /opt/campaign-python/python \
    && cp -a /tmp/release/veritas-0.1.0-linux-aarch64 /opt/veritas \
    && rm -rf /opt/veritas/evaluation /opt/veritas/examples /opt/veritas/language /opt/veritas/docs /tmp/release /tmp/veritas-linux-aarch64.tar.gz \
    && chmod -R a-w /opt/campaign-python /opt/veritas

FROM ${PYTHON_BASE} AS python-runner
ARG UV_VERSION
COPY --from=release-extract /opt/campaign-python /opt/campaign-python
ENV UV_PYTHON_INSTALL_DIR=/opt/campaign-python/python

COPY uv /usr/local/bin/uv
COPY campaign-dependencies.tar.gz /tmp/campaign-dependencies.tar.gz
RUN chmod 0555 /usr/local/bin/uv \
    && mkdir -p /opt \
    && python3 -c 'import tarfile; tarfile.open("/tmp/campaign-dependencies.tar.gz", "r:gz").extractall("/opt", filter="data")' \
    && rm /tmp/campaign-dependencies.tar.gz \
    && uv --version | grep -F "uv ${UV_VERSION}"

COPY case_runner.py /opt/runner/case_runner.py
COPY manifest.json /opt/runner/manifest.json
COPY candidate-python.json /opt/runner/candidate.json
RUN chmod 0400 /opt/runner/case_runner.py /opt/runner/manifest.json /opt/runner/candidate.json \
    && chmod 0500 /opt/runner

ENTRYPOINT ["python3", "/opt/runner/case_runner.py"]

FROM python-runner AS veritas-runner
COPY candidate-veritas.json /opt/runner/candidate.json
COPY --from=release-extract /opt/veritas /opt/veritas
RUN chmod 0400 /opt/runner/candidate.json
