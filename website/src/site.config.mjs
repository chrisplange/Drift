// Site-wide settings.
export const site = {
  name: "Drift",
  tagline: "Precise, parametric 2D sketching",
  // The site's public address, from the SITE_URL environment variable (the Website workflow
  // passes the repository variable of the same name). Pages use it for their canonical link
  // and share tags; without it, those tags are left out.
  url: process.env.SITE_URL?.replace(/\/+$/, "") || undefined,
  repo: "chrisplange/Drift",
  // The release the front page's "What's new" section describes. Until it's published on
  // GitHub, the section says it's coming; after, it says it's out.
  nextRelease: { version: "0.1.0-beta.1.3", name: "Beta 1.3" },
  get repoUrl() {
    return `https://github.com/${this.repo}`;
  },
};
