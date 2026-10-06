# tern landing page

Plain static HTML and CSS, no build step. The logo is `assets/icon.svg`, used by every page element and the favicon, so replacing that one file changes it everywhere.

Preview: `python3 -m http.server -d site` then open http://localhost:8000.

Deploy: `site/deploy.sh` (rsync to grape-2). It serves from the `carrot-soft-tern` nginx container behind Traefik at https://tern.carrot-soft.tech.
