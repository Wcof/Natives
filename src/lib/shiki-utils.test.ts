import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { extToLanguage, detectLanguage } from './shiki-utils';

describe('ShikiUtils', () => {
  describe('extToLanguage', () => {
    it('should map common extensions', () => {
      assert.equal(extToLanguage('ts'), 'typescript');
      assert.equal(extToLanguage('tsx'), 'tsx');
      assert.equal(extToLanguage('js'), 'javascript');
      assert.equal(extToLanguage('jsx'), 'jsx');
      assert.equal(extToLanguage('py'), 'python');
      assert.equal(extToLanguage('rs'), 'rust');
      assert.equal(extToLanguage('go'), 'go');
    });

    it('should map web extensions', () => {
      assert.equal(extToLanguage('html'), 'html');
      assert.equal(extToLanguage('css'), 'css');
      assert.equal(extToLanguage('scss'), 'scss');
      assert.equal(extToLanguage('json'), 'json');
    });

    it('should return text for unknown extensions', () => {
      assert.equal(extToLanguage('xyz'), 'text');
      assert.equal(extToLanguage(''), 'text');
    });
  });

  describe('detectLanguage', () => {
    it('should detect language from filename', () => {
      assert.equal(detectLanguage('app.ts'), 'typescript');
      assert.equal(detectLanguage('index.tsx'), 'tsx');
      assert.equal(detectLanguage('main.py'), 'python');
      assert.equal(detectLanguage('style.css'), 'css');
    });

    it('should handle filenames without extension', () => {
      assert.equal(detectLanguage('Makefile'), 'text');
      assert.equal(detectLanguage('.gitignore'), 'text');
    });

    it('should handle filenames with multiple dots', () => {
      assert.equal(detectLanguage('app.test.ts'), 'typescript');
      assert.equal(detectLanguage('config.d.ts'), 'typescript');
    });
  });
});
