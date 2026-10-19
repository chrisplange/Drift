// Drift's releases, read from the GitHub API when the site is built. The release steps in
// docs/releasing.md rebuild the site once a release is published, so these pages stay current
// with no edits; the download page re-checks in the visitor's browser as well.
import { site } from "../site.config.mjs";

export interface Asset {
  name: string;
  url: string;
  size: number;
}

export interface Release {
  tag: string;
  name: string;
  version: string;
  prerelease: boolean;
  published: string;
  url: string;
  html: string;
  assets: Asset[];
}

export type Platform = "windows-installer" | "windows-zip" | "macos";

// Keep in step with the names in docs/releasing.md.
export const assetSuffix: Record<Platform, string> = {
  "windows-installer": "-windows-x64-setup.exe",
  "windows-zip": "-windows-x64.zip",
  macos: "-macos-universal.dmg",
};

let cache: Promise<Release[]> | undefined;

export function getReleases(): Promise<Release[]> {
  cache ??= load();
  return cache;
}

async function load(): Promise<Release[]> {
  const headers: Record<string, string> = {
    Accept: "application/vnd.github.html+json",
    "X-GitHub-Api-Version": "2022-11-28",
    "User-Agent": "drift-website",
  };
  const token = process.env.GITHUB_TOKEN;
  if (token) headers.Authorization = `Bearer ${token}`;
  try {
    const res = await fetch(`https://api.github.com/repos/${site.repo}/releases?per_page=30`, {
      headers,
    });
    if (!res.ok) throw new Error(`GitHub API: ${res.status} ${res.statusText}`);
    const data = (await res.json()) as any[];
    return data
      .filter((r) => !r.draft)
      .map((r) => ({
        tag: r.tag_name,
        name: r.name || r.tag_name,
        version: String(r.tag_name).replace(/^v/, ""),
        prerelease: r.prerelease,
        published: r.published_at,
        url: r.html_url,
        html: r.body_html ?? "",
        assets: (r.assets ?? []).map((a: any) => ({
          name: a.name,
          url: a.browser_download_url,
          size: a.size,
        })),
      }));
  } catch (err) {
    // An offline or rate-limited build still produces a site; the pages link to GitHub.
    console.warn(`[releases] couldn't read releases: ${err}`);
    return [];
  }
}

export function findAsset(release: Release | undefined, platform: Platform): Asset | undefined {
  return release?.assets.find((a) => a.name.endsWith(assetSuffix[platform]));
}

/** A version as people say it: "0.1.0-beta.1.1" is "Beta 1.1", "0.1.0-alpha.7" is "Alpha 7". */
export function friendlyVersion(version: string): string {
  const m = version.match(/-(alpha|beta|rc)\.?(.*)$/i);
  if (!m) return version;
  const stage = m[1].toLowerCase() === "rc" ? "Release candidate" : m[1][0].toUpperCase() + m[1].slice(1);
  return m[2] ? `${stage} ${m[2]}` : stage;
}

/** The release stage a version belongs to, for "Drift is … software". */
export function stage(version: string | undefined): string {
  const m = version?.match(/-(alpha|beta|rc)/i);
  return m ? (m[1].toLowerCase() === "rc" ? "release-candidate" : m[1].toLowerCase()) : "";
}

export function formatSize(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
}

export function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "long",
    year: "numeric",
  });
}
