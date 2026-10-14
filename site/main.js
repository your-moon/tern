// Behaviour for the landing page. Every word on the page lives in index.html;
// this file only chooses what to show and keeps links current.

const REPO = "your-moon/tern";
const DOWNLOAD_BASE = `https://github.com/${REPO}/releases/latest/download`;

// Which platform block to show in the hero: "mac", "windows", "linux" or "other".
// `?os=windows` (or mac, linux, other) forces one, for testing or a shared link.
function detectPlatform() {
  const forced = new URLSearchParams(location.search).get("os");
  if (["mac", "windows", "linux", "other"].includes(forced)) return forced;

  const platform = navigator.userAgentData?.platform || navigator.platform || "";
  const agent = navigator.userAgent;
  if (/iphone|ipad|android/i.test(agent)) return "other";
  if (/win/i.test(platform)) return "windows";
  if (/mac/i.test(platform)) return "mac";
  if (/linux|x11/i.test(platform)) return "linux";
  return "other";
}

// Shows the elements marked with this platform and hides the other platforms'.
function showPlatform(platform) {
  for (const el of document.querySelectorAll("[data-platform]")) {
    el.hidden = el.dataset.platform !== platform;
  }
}

// Points every download link at the latest release's file names, and shows its
// version, so the page never links to a release that has been replaced.
async function followLatestRelease() {
  const response = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`);
  if (!response.ok) return;
  const release = await response.json();
  if (!/^v\d+\.\d+\.\d+$/.test(release.tag_name)) return;

  const version = release.tag_name.slice(1);
  for (const link of document.querySelectorAll("a[data-asset]")) {
    link.href = `${DOWNLOAD_BASE}/tern-${version}-${link.dataset.asset}`;
  }
  for (const el of document.querySelectorAll(".release-version")) {
    el.textContent = release.tag_name;
  }
}

// Shows the repository's star count next to the GitHub links.
async function showStarCount() {
  const response = await fetch(`https://api.github.com/repos/${REPO}`);
  if (!response.ok) return;
  const { stargazers_count: stars } = await response.json();
  if (typeof stars !== "number") return;

  const text = stars >= 1000 ? `${(stars / 1000).toFixed(1).replace(/\.0$/, "")}k` : String(stars);
  for (const el of document.querySelectorAll(".star-count")) {
    el.textContent = `★ ${text}`;
    el.hidden = false;
  }
}

// Tabs above a framed screenshot: one panel visible at a time; arrow keys move between tabs.
function setUpTabs(tablist) {
  const tabs = [...tablist.querySelectorAll('[role="tab"]')];

  function select(tab) {
    for (const t of tabs) {
      const selected = t === tab;
      t.setAttribute("aria-selected", String(selected));
      t.tabIndex = selected ? 0 : -1;
      document.getElementById(t.getAttribute("aria-controls")).hidden = !selected;
    }
  }

  tabs.forEach((tab, i) => {
    tab.tabIndex = i === 0 ? 0 : -1;
    tab.addEventListener("click", () => select(tab));
    tab.addEventListener("keydown", (event) => {
      const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
      if (!step) return;
      const next = tabs[(i + step + tabs.length) % tabs.length];
      select(next);
      next.focus();
      event.preventDefault();
    });
  });
}

showPlatform(detectPlatform());
document.querySelectorAll('[role="tablist"]').forEach(setUpTabs);
// Both requests are optional: if GitHub doesn't answer, the static links and text stay.
followLatestRelease().catch(() => {});
showStarCount().catch(() => {});
