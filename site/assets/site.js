import { allDocFields, docsCatalog, pageOrder } from "./docs-catalog.mjs";

const page = document.body.dataset.page || "overview";
const topbar = document.getElementById("topbar");
const sidebar = document.getElementById("sidebar");
const content = document.getElementById("content");

const pageLinks = [
  { href: "index.html", label: "Overview", key: "overview" },
  ...pageOrder.map((kind) => ({
    href: `${kind}.html`,
    label: docsCatalog[kind].pageTitle,
    key: kind,
  })),
];

const fieldCountFor = (kind) =>
  docsCatalog[kind].sections.reduce((count, section) => count + section.fields.length, 0);

const escapeHtml = (value) =>
  String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");

const renderPills = (values, className = "pill") =>
  values.map((value) => `<span class="${className}">${escapeHtml(value)}</span>`).join("");

const renderFieldCard = (field) => {
  const values = field.values ?? [];
  return `
    <article class="field-card surface-card" id="${field.slug}">
      <div class="field-card-header">
        <div>
          <p class="field-path">${escapeHtml(field.fullPath)}</p>
          <h3>${escapeHtml(field.title)}</h3>
        </div>
        <span class="pill pill-strong">default: ${escapeHtml(field.defaultValue)}</span>
      </div>
      <p class="field-summary">${escapeHtml(field.summary)}</p>
      <div class="field-media" data-scene="${escapeHtml(field.compositionId)}">
        <video class="scene-video" muted loop playsinline preload="none" aria-label="Animation for ${escapeHtml(field.title)}">
          <source src="${escapeHtml(field.assetPath)}" type="video/mp4" />
        </video>
        <div class="field-media-overlay">
          <span class="media-tag">Remotion scene</span>
          <span class="media-template">${escapeHtml(field.scene.template)}</span>
        </div>
        <div class="field-media-fallback">Animation renders in CI for GitHub Pages. If the MP4 is missing locally, this placeholder stays visible.</div>
      </div>
      <p class="field-behavior">${escapeHtml(field.behavior)}</p>
      <div class="pill-row">${renderPills(values)}</div>
      ${field.note ? `<p class="field-note">${escapeHtml(field.note)}</p>` : ""}
    </article>
  `;
};

const renderTopbar = () => {
  if (!topbar)return;
  topbar.innerHTML = `
    <div class="brand-row">
      <a class="brand-link" href="index.html">yeetnyoink</a>
      <p class="brand-caption">Config-driven docs and motion explanations for browser, editor, and terminal routing.</p>
    </div>
    <nav class="page-nav" aria-label="Primary docs navigation">
      ${pageLinks
        .map(
          (link) => `
            <a class="page-nav-link ${link.key === page ? "is-active" : ""}" href="${link.href}">${escapeHtml(link.label)}</a>
          `,
        )
        .join("")}
    </nav>
  `;
};

const renderOverview = () => {
  if (!content||!sidebar)return;
  sidebar.innerHTML = `
    <div class="sidebar-block surface-card">
      <span class="eyebrow">Pages</span>
      <h2>Choose a profile family</h2>
      <p>The overview stays high-level. The three subpages drill all the way down to the individual config fields and their matching scenes.</p>
      <div class="sidebar-page-list">
        ${pageOrder
          .map(
            (kind) => `
              <a class="sidebar-page-link" href="${kind}.html">
                <strong>${escapeHtml(docsCatalog[kind].pageTitle)}</strong>
                <span>${fieldCountFor(kind)} documented fields</span>
              </a>
            `,
          )
          .join("")}
      </div>
    </div>
    <div class="sidebar-block surface-card compact">
      <h2>Inventory</h2>
      <ul>
        <li>${allDocFields.length} config-field scenes in the catalog</li>
        <li>1 overview page + 3 kind-specific subpages</li>
        <li>Shared data source for docs and Remotion registrations</li>
      </ul>
    </div>
  `;

  content.innerHTML = `
    <section class="hero surface-card hero-overview">
      <span class="eyebrow">User documentation</span>
      <h1>Follow the config surface one field at a time.</h1>
      <p class="lede">
        The public docs now split the product by app kind. Each page walks the real config surface from <code>src/config.rs</code>
        and pairs every documented browser, editor, and terminal field with a matching Remotion scene.
      </p>
      <div class="hero-actions">
        ${pageOrder
          .map(
            (kind) => `
              <a class="button primary" href="${kind}.html">Open ${escapeHtml(docsCatalog[kind].pageTitle)}</a>
            `,
          )
          .join("")}
      </div>
    </section>

    <section class="overview-grid">
      ${pageOrder
        .map((kind) => {
          const doc = docsCatalog[kind];
          return `
            <article class="surface-card overview-card">
              <span class="eyebrow">${escapeHtml(doc.label)}</span>
              <h2>${escapeHtml(doc.heroTitle)}</h2>
              <p>${escapeHtml(doc.heroIntro)}</p>
              <div class="metric-row">
                <span class="metric-pill">${fieldCountFor(kind)} fields</span>
                <span class="metric-pill">${doc.sections.length} sections</span>
              </div>
              <pre><code>${escapeHtml(doc.sampleConfig)}</code></pre>
              <p><a class="button secondary" href="${kind}.html">Read the ${escapeHtml(kind)} guide</a></p>
            </article>
          `;
        })
        .join("")}
    </section>

    <section class="surface-card prose-card">
      <span class="eyebrow">How to read the pages</span>
      <h2>Each field entry is both a docs card and an animation target.</h2>
      <ul class="prose-list">
        <li>The shared catalog drives both the written explanation and the Remotion composition registry.</li>
        <li>Direction overrides are documented individually, so you can reason about one axis at a time.</li>
        <li>Shared concepts like tear-out, snap-back, and allowed directions reuse a uniform visual language across app kinds.</li>
      </ul>
    </section>
  `;
};

const renderKindPage = (kind) => {
  if (!content||!sidebar||!docsCatalog[kind])return;
  const doc = docsCatalog[kind];
  sidebar.innerHTML = `
    <div class="sidebar-block surface-card">
      <span class="eyebrow">${escapeHtml(doc.label)}</span>
      <h2>${escapeHtml(doc.pageTitle)}</h2>
      <p>${escapeHtml(doc.heroIntro)}</p>
      <div class="metric-row">
        <span class="metric-pill">${fieldCountFor(kind)} fields</span>
        <span class="metric-pill">${doc.sections.length} sections</span>
      </div>
    </div>
    <nav class="sidebar-block surface-card section-nav" aria-label="Sections on this page">
      <span class="eyebrow">Sections</span>
      ${doc.sections
        .map(
          (section) => `<a class="section-link" href="#${section.id}">${escapeHtml(section.title)}</a>`,
        )
        .join("")}
    </nav>
  `;

  content.innerHTML = `
    <section class="hero surface-card">
      <span class="eyebrow">${escapeHtml(doc.label)}</span>
      <h1>${escapeHtml(doc.heroTitle)}</h1>
      <p class="lede">${escapeHtml(doc.heroIntro)}</p>
      <div class="hero-two-up">
        <div class="surface-subcard">
          <h2>Sample config</h2>
          <pre><code>${escapeHtml(doc.sampleConfig)}</code></pre>
        </div>
        <div class="surface-subcard">
          <h2>Page structure</h2>
          <ul class="prose-list compact-list">
            ${doc.sections
              .map(
                (section) => `<li><a href="#${section.id}">${escapeHtml(section.title)}</a> · ${section.fields.length} fields</li>`,
              )
              .join("")}
          </ul>
        </div>
      </div>
    </section>

    ${doc.sections
      .map(
        (section) => `
          <section class="docs-section" id="${section.id}">
            <div class="section-heading">
              <span class="eyebrow">${escapeHtml(doc.label)}</span>
              <h2>${escapeHtml(section.title)}</h2>
              <p>${escapeHtml(section.blurb)}</p>
            </div>
            <div class="field-grid">
              ${section.fields.map((field) => renderFieldCard(field)).join("")}
            </div>
          </section>
        `,
      )
      .join("")}
  `;
};

const installVideoObservers = () => {
  const videos = Array.from(document.querySelectorAll(".scene-video"));
  if (!videos.length)return;

  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        const video = entry.target;
        if (entry.isIntersecting) {
          video.play().catch(() => {});
        } else {
          video.pause();
        }
      });
    },
    { threshold: 0.45 },
  );

  videos.forEach((video) => {
    video.addEventListener("error", () => {
      video.closest(".field-media")?.classList.add("is-missing");
    });
    observer.observe(video);
  });
};

const installSectionObserver = () => {
  const links = Array.from(document.querySelectorAll(".section-link"));
  const sections = links
    .map((link) => document.querySelector(link.getAttribute("href")))
    .filter(Boolean);

  if (!links.length||!sections.length)return;

  const updateActive = (id) => {
    links.forEach((link) => {
      link.classList.toggle("is-active", link.getAttribute("href") === `#${id}`);
    });
  };

  const observer = new IntersectionObserver(
    (entries) => {
      const visible = entries
        .filter((entry) => entry.isIntersecting)
        .sort((a, b) => b.intersectionRatio - a.intersectionRatio)[0];
      if (visible?.target?.id) {
        updateActive(visible.target.id);
      }
    },
    { rootMargin: "-18% 0px -62% 0px", threshold: [0.2, 0.45, 0.7] },
  );

  sections.forEach((section) => observer.observe(section));
  updateActive(sections[0].id);
};

renderTopbar();
if (page === "overview") {
  renderOverview();
} else {
  renderKindPage(page);
}
installVideoObservers();
installSectionObserver();
