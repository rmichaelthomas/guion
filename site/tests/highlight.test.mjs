import test from 'node:test';
import assert from 'node:assert/strict';
import { highlightGuion } from '../tools/highlight.mjs';

test('keywords, types, strings and numbers are wrapped', () => {
  const html = highlightGuion('fn main() -> Int:\n    let greeting = "Hello, Guion!"\n    return 0\n');
  assert.ok(html.includes('<span class="k">fn</span> main() -&gt; <span class="t">Int</span>:'));
  assert.ok(html.includes('<span class="k">let</span> greeting = <span class="s">"Hello, Guion!"</span>'));
  assert.ok(html.includes('<span class="k">return</span> <span class="n">0</span>'));
});

test('a comment runs to the end of the line and keeps its words unstyled', () => {
  const html = highlightGuion('let x = 1  # let it be\n');
  assert.ok(html.includes('<span class="c"># let it be</span>'));
  assert.ok(!html.includes('<span class="k">let</span> it'));
});

test('a # inside a string is not a comment', () => {
  const html = highlightGuion('print("a # b")\n');
  assert.ok(html.includes('<span class="s">"a # b"</span>'));
  assert.ok(!html.includes('class="c"'));
});

test('markup in the source is escaped', () => {
  const html = highlightGuion('print("<b>&</b>")\n');
  assert.ok(html.includes('&lt;b&gt;&amp;&lt;/b&gt;'));
  assert.ok(!html.includes('<b>'));
});

test('the text is unchanged once tags are removed', () => {
  const source = 'record Point:\n    x: Int\n\nfn main() -> Int:\n    return Some(3)\n';
  const plain = highlightGuion(source).replace(/<[^>]+>/g, '').replaceAll('&gt;', '>').replaceAll('&lt;', '<').replaceAll('&amp;', '&');
  assert.equal(plain, source);
});
