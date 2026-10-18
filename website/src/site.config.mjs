// Site-wide settings. Change the domain here once it's chosen; nothing else hard-codes it.
export const site = {
  name: "Drift",
  tagline: "Parametric 2D sketching for designers",
  url: "https://drift-website.pages.dev",
  repo: "chrisplange/Drift",
  get repoUrl() {
    return `https://github.com/${this.repo}`;
  },
};
