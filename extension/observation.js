// Kept self-contained: executeScript serializes this function, not its module scope.
export function extractAccountObservation() {
  try {
    const page = new URL(location.href);
    if (page.protocol !== 'https:' || page.hostname !== 'www.fiverr.com'
        || page.port !== '' || page.username !== '' || page.password !== ''
        || page.search !== '' || page.hash !== '') return null;

    const managementPath = /^\/users\/(?:([A-Za-z0-9_-]{1,80})\/)?manage_gigs\/?$/;
    const route = managementPath.exec(page.pathname);
    if (!route || !document.body) return null;
    if (document.querySelector('.g-recaptcha, #challenge-form, iframe[src*="captcha"], form[action*="challenge"], form[action*="login"], input[type="password"]')) return null;

    const sameOrigin = (anchor) => {
      try {
        const url = new URL(anchor.getAttribute('href'), page.href);
        return url.origin === page.origin && url.username === '' && url.password === ''
          && url.search === '' && url.hash === ''
          ? url : null;
      } catch {
        return null;
      }
    };
    const navigation = document.querySelectorAll(
      'nav a[href], header a[href], [role="navigation"] a[href]',
    );
    if (navigation.length > 1000) return null;
    const usernames = new Set();
    let logout = false;
    for (const anchor of navigation) {
      const url = sameOrigin(anchor);
      if (!url) continue;
      const profile = /^\/sellers\/([A-Za-z0-9_-]{1,80})\/edit$/.exec(url.pathname);
      if (profile) usernames.add(profile[1]);
      if (/^\/(?:users\/)?logout\/?$/.test(url.pathname)) logout = true;
    }
    if (usernames.size !== 1 || !logout) return null;
    const username = usernames.values().next().value;
    if (route[1] && route[1] !== username) return null;

    if (!document.body.querySelector('.js-db-table')) return null;
    return { type: 'account_observation', url: location.href, username, management: true, logout: true };
  } catch {
    // Missing or changed markup is unknown, never an empty successful account.
    return null;
  }
}
