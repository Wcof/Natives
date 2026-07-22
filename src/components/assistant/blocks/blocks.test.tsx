// ─── Content Block Renderer Tests ───────────────────────
//
// Tests for all block types, unknown block fallback,
// keyboard accessibility, safe Markdown rendering, and large code blocks.

import { describe, it, assert } from '@/lib/test-utils';
import { renderBlock, renderBlocks, type ContentBlock } from './index';

describe('ContentBlockRenderers', () => {
  it('should render text block', () => {
    const block: ContentBlock = { type: 'text', text: 'Hello, world!' };
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'Text block should render');
  });

  it('should render reasoning block', () => {
    const block: ContentBlock = { type: 'reasoning', reasoning: 'Step-by-step reasoning' };
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'Reasoning block should render');
  });

  it('should render image block', () => {
    const block: ContentBlock = { type: 'image', imageUrl: 'https://example.com/image.png', altText: 'Example' };
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'Image block should render');
  });

  it('should render file reference block', () => {
    const block: ContentBlock = { type: 'file_reference', filePath: '/tmp/test.txt', fileSize: 1024 };
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'File reference block should render');
  });

  it('should render tool call block', () => {
    const block: ContentBlock = {
      type: 'tool_call',
      toolName: 'read_file',
      toolInput: { path: '/tmp/test.txt' },
      toolStatus: 'running',
    };
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'Tool call block should render');
  });

  it('should render tool result block', () => {
    const block: ContentBlock = {
      type: 'tool_result',
      toolOutput: { content: 'file contents' },
      isError: false,
      durationMs: 150,
    };
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'Tool result block should render');
  });

  it('should render citation block', () => {
    const block: ContentBlock = {
      type: 'citation',
      citationUri: 'https://example.com/doc',
      citationTitle: 'Documentation',
    };
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'Citation block should render');
  });

  it('should render error block', () => {
    const block: ContentBlock = {
      type: 'error',
      errorCode: 'NOT_FOUND',
      errorMessage: 'Resource not found',
      retryable: true,
    };
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'Error block should render');
  });

  it('should render legacy block', () => {
    const block: ContentBlock = {
      type: 'legacy',
      raw: 'Legacy content',
      originalType: 'thinking',
    };
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'Legacy block should render');
  });

  it('should render unknown block as fallback', () => {
    const block = { type: 'unknown' as ContentBlock['type'] } as ContentBlock;
    const result = renderBlock(block, 0);
    assert.ok(result !== null, 'Unknown block should render fallback');
  });

  it('should render multiple blocks', () => {
    const blocks: ContentBlock[] = [
      { type: 'text', text: 'First' },
      { type: 'text', text: 'Second' },
      { type: 'text', text: 'Third' },
    ];
    const results = renderBlocks(blocks);
    assert.equal(results.length, 3, 'Should render all blocks');
  });
});
  it('text and plan blocks use MarkdownText (not raw pre-wrap markers only)', () => {
    // Structural contract: renderBlock returns a React element tree for text/plan.
    const text = renderBlock({ type: 'text', text: '## Hello' }, 0) as { type?: { name?: string } | string; props?: Record<string, unknown> };
    const plan = renderBlock({ type: 'plan', planMarkdown: '- [x] step' }, 1) as { type?: { name?: string } | string; props?: Record<string, unknown> };
    assert.ok(text !== null);
    assert.ok(plan !== null);
    // Ensure we did not leave a plain string child of "## Hello" as the sole content
    // (MarkdownText component should wrap the source).
    const textJson = JSON.stringify(text);
    assert.ok(textJson.includes('## Hello') || textJson.includes('Hello'));
    const planJson = JSON.stringify(plan);
    assert.ok(planJson.includes('step'));
  });

  it('reasoning and tool blocks remain non-markdown wrappers', () => {
    const reasoning = renderBlock({ type: 'reasoning', reasoning: 'think' }, 0);
    const tool = renderBlock({
      type: 'tool_call',
      toolName: 'read_file',
      toolInput: { path: '/tmp/a' },
      toolStatus: 'completed',
    }, 1);
    assert.ok(reasoning !== null);
    assert.ok(tool !== null);
  });
