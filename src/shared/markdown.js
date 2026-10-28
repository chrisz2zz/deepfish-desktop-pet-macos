(function exposeMarkdown(root, factory) {
  const api = factory();
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  if (root) root.DeepFishMarkdown = api;
})(typeof window !== "undefined" ? window : globalThis, function createMarkdown() {
  // Deliberately small and strict: the output is built from escaped text and a
  // fixed set of tags, never from the model's raw HTML. Links are limited to
  // http(s) so a reply cannot smuggle `javascript:` or `file:` targets into the
  // webview.
  const HTML_ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
  const LINK_LIMIT = 2048;
  const FENCE = /^\s*```\s*([\w+#.-]*)\s*$/;
  const FENCE_END = /^\s*```\s*$/;
  const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
  const LIST_ITEM = /^\s{0,3}([-*+]|\d{1,9}[.)])\s+(.*)$/;
  const BLOCK_START = /^\s{0,3}(#{1,6}\s|>|```)/;
  const RULE = /^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/;
  const TRAILING_PUNCT = /[.,;:!?，。；：！？、]+$/;

  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, (char) => HTML_ESCAPES[char]);
  }

  function safeHref(raw) {
    const href = String(raw).trim();
    if (!href || href.length > LINK_LIMIT) return null;
    if (/[\u0000-\u0020\u007f]/.test(href)) return null;
    if (!/^https?:\/\//i.test(href)) return null;
    return href;
  }

  function anchor(href, labelHtml) {
    return `<a href="${href}" data-external="1" rel="noreferrer">${labelHtml}</a>`;
  }

  function emphasis(text) {
    return text
      .replace(/\*\*\*([^*]+)\*\*\*/g, "<strong><em>$1</em></strong>")
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/(^|[^*])\*([^*\n]+)\*(?!\*)/g, "$1<em>$2</em>")
      .replace(/~~([^~]+)~~/g, "<del>$1</del>")
      .replace(/(^|[\s(])_([^_\n]+)_(?=$|[\s).,!?:;])/g, "$1<em>$2</em>");
  }

  /// Escapes one run of inline text and applies links, bare URLs and emphasis.
  /// Placeholders keep generated anchors out of the emphasis pass so underscores
  /// and asterisks inside a URL cannot rewrite the tag it lives in.
  function formatText(raw) {
    const links = [];
    const hold = (href) => {
      links.push(href);
      return `\u0000${links.length - 1}\u0000`;
    };

    let text = escapeHtml(raw);

    text = text.replace(/\[([^\]]*)\]\(([^)\s]+)\)/g, (whole, label, href) => {
      const url = safeHref(href);
      if (!url) return whole;
      return hold({ url, label });
    });

    text = text.replace(/(^|[\s(（])(https?:\/\/[^\s<>"）)]+)/gi, (whole, prefix, url) => {
      const trailing = (url.match(TRAILING_PUNCT) || [""])[0];
      const clean = trailing ? url.slice(0, -trailing.length) : url;
      if (!safeHref(clean)) return whole;
      return `${prefix}${hold({ url: clean, label: clean })}${trailing}`;
    });

    text = emphasis(text).replace(/\n/g, "<br>");
    return text.replace(/\u0000(\d+)\u0000/g, (whole, index) => {
      const entry = links[Number(index)];
      return entry ? anchor(entry.url, emphasis(entry.label)) : whole;
    });
  }

  /// Splits code spans out before inline formatting so their content stays
  /// literal (`a * b` inside backticks must not turn italic).
  function inline(raw) {
    let rest = String(raw).replace(/\u0000/g, "");
    let html = "";
    for (;;) {
      const match = /`([^`\n]+)`/.exec(rest);
      if (!match) break;
      html += formatText(rest.slice(0, match.index));
      html += `<code>${escapeHtml(match[1].trim())}</code>`;
      rest = rest.slice(match.index + match[0].length);
    }
    return html + formatText(rest);
  }

  function isOrdered(marker) {
    return /\d/.test(marker[0]);
  }

  function renderBlocks(lines) {
    const out = [];
    let index = 0;
    while (index < lines.length) {
      const line = lines[index];
      if (!line.trim()) {
        index += 1;
        continue;
      }

      const fence = line.match(FENCE);
      if (fence) {
        const language = fence[1];
        const code = [];
        index += 1;
        while (index < lines.length && !FENCE_END.test(lines[index])) {
          code.push(lines[index]);
          index += 1;
        }
        if (index < lines.length) index += 1;
        const className = language ? ` class="language-${language}"` : "";
        out.push(`<pre><code${className}>${escapeHtml(code.join("\n"))}</code></pre>`);
        continue;
      }

      const heading = line.match(HEADING);
      if (heading) {
        const level = heading[1].length;
        out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
        index += 1;
        continue;
      }

      if (RULE.test(line)) {
        out.push("<hr>");
        index += 1;
        continue;
      }

      if (/^\s{0,3}>\s?/.test(line)) {
        const quoted = [];
        while (index < lines.length && /^\s{0,3}>\s?/.test(lines[index])) {
          quoted.push(lines[index].replace(/^\s{0,3}>\s?/, ""));
          index += 1;
        }
        out.push(`<blockquote>${renderBlocks(quoted)}</blockquote>`);
        continue;
      }

      const item = line.match(LIST_ITEM);
      if (item) {
        const ordered = isOrdered(item[1]);
        const items = [];
        while (index < lines.length) {
          const current = lines[index].match(LIST_ITEM);
          if (!current || isOrdered(current[1]) !== ordered) break;
          let content = current[2];
          index += 1;
          // A wrapped item continues on the following non-blank lines until
          // another list entry or block starts.
          while (index < lines.length) {
            const next = lines[index];
            if (!next.trim() || LIST_ITEM.test(next) || BLOCK_START.test(next) || RULE.test(next)) break;
            content += ` ${next.trim()}`;
            index += 1;
          }
          items.push(`<li>${inline(content)}</li>`);
        }
        out.push(ordered ? `<ol>${items.join("")}</ol>` : `<ul>${items.join("")}</ul>`);
        continue;
      }

      const paragraph = [];
      while (index < lines.length && lines[index].trim()) {
        const next = lines[index];
        if (BLOCK_START.test(next) || LIST_ITEM.test(next) || RULE.test(next)) break;
        paragraph.push(next);
        index += 1;
      }
      out.push(`<p>${inline(paragraph.join("\n"))}</p>`);
    }
    return out.join("");
  }

  function render(source) {
    if (typeof source !== "string" || !source.trim()) return "";
    return renderBlocks(source.replace(/\r\n?/g, "\n").split("\n"));
  }

  return { render };
});
