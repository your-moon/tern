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

// GitHub's API allows 60 requests an hour per network, which a shared office or
// VPN uses up quickly. shields.io serves the same numbers from its own cache, so it
// is asked first; the GitHub API is only the fallback.
async function fetchShield(path) {
  const response = await fetch(`https://img.shields.io/github/${path}/${REPO}.json`);
  if (!response.ok) return null;
  const { value } = await response.json();
  return value && value !== "invalid" ? value : null;
}

async function fetchGitHub(path) {
  const response = await fetch(`https://api.github.com/repos/${REPO}${path}`);
  return response.ok ? response.json() : null;
}

// The latest release tag, like "v0.1.3", or null if neither source answers.
async function latestReleaseTag() {
  const tag = (await fetchShield("v/release").catch(() => null))
    ?? (await fetchGitHub("/releases/latest").catch(() => null))?.tag_name;
  return /^v\d+\.\d+\.\d+$/.test(tag ?? "") ? tag : null;
}

// The star count as text, like "37" or "1.2k", or null if neither source answers.
async function starCountText() {
  const shown = await fetchShield("stars").catch(() => null);
  if (shown) return shown;
  const stars = (await fetchGitHub("").catch(() => null))?.stargazers_count;
  if (typeof stars !== "number") return null;
  return stars >= 1000 ? `${(stars / 1000).toFixed(1).replace(/\.0$/, "")}k` : String(stars);
}

// Points every download link at the latest release's file names, and shows its
// version, so the page never links to a release that has been replaced.
async function followLatestRelease() {
  const tag = await latestReleaseTag();
  if (!tag) return;
  const version = tag.slice(1);
  for (const link of document.querySelectorAll("a[data-asset]")) {
    link.href = `${DOWNLOAD_BASE}/tern-${version}-${link.dataset.asset}`;
  }
  for (const el of document.querySelectorAll(".release-version")) {
    el.textContent = tag;
  }
}

// Shows the repository's star count next to the GitHub links.
async function showStarCount() {
  const text = await starCountText();
  if (!text) return;
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
// Both are optional: if no source answers, the static links and text stay.
followLatestRelease().catch(() => {});
showStarCount().catch(() => {});
