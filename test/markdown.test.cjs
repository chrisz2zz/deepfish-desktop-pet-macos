const test = require("node:test");
const assert = require("node:assert/strict");
const markdown = require("../src/shared/markdown.js");

test("renders headings, emphasis and inline code", () => {
  const html = markdown.render("# 标题\n\n**粗体**、*斜体*、~~删除~~ 和 `a * b`");
  assert.equal(
    html,
    "<h1>标题</h1><p><strong>粗体</strong>、<em>斜体</em>、<del>删除</del> 和 <code>a * b</code></p>"
  );
});

test("renders fenced code literally", () => {
  const html = markdown.render("```js\nconst a = 1 * 2;\n```");
  assert.equal(html, '<pre><code class="language-js">const a = 1 * 2;</code></pre>');
});

test("never formats the contents of a code span", () => {
  assert.equal(markdown.render("`**不是粗体**`"), "<p><code>**不是粗体**</code></p>");
});

test("renders lists, wrapped items and quotes", () => {
  const html = markdown.render("- 第一项\n- 第二项\n 继续\n\n1. 一\n2. 二\n\n> 引用\n> 第二行");
  assert.equal(
    html,
    "<ul><li>第一项</li><li>第二项 继续</li></ul>" +
      "<ol><li>一</li><li>二</li></ol>" +
      "<blockquote><p>引用<br>第二行</p></blockquote>"
  );
});

test("keeps single line breaks inside a paragraph", () => {
  assert.equal(markdown.render("第一行\n第二行"), "<p>第一行<br>第二行</p>");
});

test("escapes raw html", () => {
  assert.equal(
    markdown.render('<img src=x onerror="alert(1)">'),
    "<p>&lt;img src=x onerror=&quot;alert(1)&quot;&gt;</p>"
  );
});

test("only http(s) links become anchors", () => {
  assert.equal(
    markdown.render("[文档](https://example.com/a?b=1&c=2)"),
    '<p><a href="https://example.com/a?b=1&amp;c=2" data-external="1" rel="noreferrer">文档</a></p>'
  );

  for (const unsafe of ["[点我](javascript:alert(1))", "[文件](file:///C:/Windows/cmd.exe)"]) {
    const html = markdown.render(unsafe);
    assert.ok(!html.includes("<a"), unsafe);
    assert.ok(html.includes(unsafe), unsafe);
  }
});

test("autolinks bare urls without swallowing punctuation", () => {
  assert.equal(
    markdown.render("看 https://example.com/a_b_c。"),
    '<p>看 <a href="https://example.com/a_b_c" data-external="1" rel="noreferrer">https://example.com/a_b_c</a>。</p>'
  );
});

test("empty input renders nothing", () => {
  assert.equal(markdown.render(""), "");
  assert.equal(markdown.render("   \n\n "), "");
  assert.equal(markdown.render(undefined), "");
});
