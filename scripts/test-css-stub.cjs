// CSS Modules are a bundler concern. Under the Node test runner there is no
// bundler, so importing one throws a SyntaxError on the first selector and takes
// down the whole test file — which is why component tests that pull in a styled
// component could never run.
//
// The proxy returns the requested key as its own class name, so `styles.root`
// yields "root". That is enough for assertions about which class was chosen and
// keeps the stub from silently turning every lookup into undefined.
require.extensions['.css'] = (module) => {
  module.exports = new Proxy(
    {},
    {
      get: (_target, key) => (typeof key === 'string' ? key : undefined),
    },
  );
};
