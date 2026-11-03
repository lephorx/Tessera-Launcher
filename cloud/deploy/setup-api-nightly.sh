#!/bin/sh
# One-time setup of the nightly API next to the released one on the home server: port 8788,
# api-nightly.tesseralauncher.com through the same tunnel, the released API's env (same database
# and accounts), and a timer that installs every new build. Run from cloud/ after deploy-api.sh --env.
set -e
cd "$(dirname "$0")"
scp -q tessera-api-nightly.service tessera-api-nightly-update.service tessera-api-nightly-update.timer tessera-api-nightly-update.sh tessera-srv:/tmp/
ssh tessera-srv 'set -e
sudo install -d -m 755 -o root -g root /opt/tessera-api-nightly
sudo sed "s/^PORT=.*/PORT=8788/" /etc/tessera/api.env | sudo tee /etc/tessera/api-nightly.env >/dev/null
sudo chown root:tessera /etc/tessera/api-nightly.env && sudo chmod 640 /etc/tessera/api-nightly.env
cd /tmp
sudo install -m 755 tessera-api-nightly-update.sh /usr/local/bin/tessera-api-nightly-update
sudo install -m 644 tessera-api-nightly.service tessera-api-nightly-update.service tessera-api-nightly-update.timer /etc/systemd/system/
rm tessera-api-nightly.service tessera-api-nightly-update.service tessera-api-nightly-update.timer tessera-api-nightly-update.sh
if ! sudo grep -q api-nightly.tesseralauncher.com /etc/cloudflared/config.yml; then
  sudo sed -i "s|^  - service: http_status:404|  - hostname: api-nightly.tesseralauncher.com\n    service: http://127.0.0.1:8788\n  - service: http_status:404|" /etc/cloudflared/config.yml
  cloudflared tunnel route dns tessera-api api-nightly.tesseralauncher.com
  sudo systemctl restart cloudflared
fi
sudo systemctl daemon-reload
sudo systemctl enable -q tessera-api-nightly
sudo systemctl enable -q --now tessera-api-nightly-update.timer
sudo systemctl start tessera-api-nightly-update.service
systemctl is-active tessera-api-nightly cloudflared'
