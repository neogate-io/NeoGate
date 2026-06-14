# NeoGate Website

This directory contains the static source for the official NeoGate project website at `neogate.io`.

It is intentionally separate from `frontend/`, which is the self-hosted product console used by Docker Compose deployments. Keeping the website here lets the project website evolve with the main repository without changing the default self-hosted user experience.

## Local Preview

```bash
cd website
python3 -m http.server 4173
```

Open:

```text
http://127.0.0.1:4173
```

## Deploy

The current site is plain static HTML/CSS and can be served by any static host or Nginx container. For `neogate.io`, publish the contents of this directory as the website root.

The English homepage is `index.html`; the Chinese homepage is `zh/index.html`.

Self-hosted NeoGate deployments should continue to use the existing `frontend/` Docker build.
