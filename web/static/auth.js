const $ = id => document.getElementById(id);

export function createAuth({ api, text, onSignIn }) {
  let registrationEnabled = false;
  const login = $('login-form');
  const register = $('register-form');

  function message(value) {
    $('auth-message').textContent = value;
    $('auth-message').hidden = !value;
  }
  function show(screen, focus = false) {
    const creating = screen === 'register';
    const recovering = screen === 'recovery';
    login.hidden = creating || recovering;
    register.hidden = !creating;
    $('recovery-help').hidden = !recovering;
    $('register-link').hidden = creating || recovering || !registrationEnabled;
    $('login-link').hidden = !creating && !recovering;
    $('auth-heading').textContent = text(creating ? 'createAccount' : recovering ? 'resetPassword' : 'welcome');
    $('auth-intro').textContent = text(creating ? 'registerIntro' : recovering ? 'recoveryIntro' : 'loginIntro');
    message('');
    if (focus) $('auth-heading').focus();
  }
  function describe(error) {
    if (error.status === 409) return text('nameUnavailable');
    if (error.status === 429) return text('authBusy');
    if (error.status === 401) return text('incorrectCredentials');
    if (error.status === 400) return text('invalidRegistration');
    if (error.status === 503) return text('authUnavailable');
    return error.message;
  }
  function busy(form, pending) { form.querySelector('button[type="submit"], button:not([type])').disabled = pending; }

  $('register-link').addEventListener('click', () => show('register', true));
  $('login-link').addEventListener('click', () => show('login', true));
  $('forgot-password').addEventListener('click', () => show('recovery', true));
  login.addEventListener('submit', async event => {
    event.preventDefault();
    message(''); busy(login, true);
    try {
      await api('/api/login', { method:'POST', body:JSON.stringify(Object.fromEntries(new FormData(login))) });
      login.reset(); await onSignIn();
    } catch (error) { message(describe(error)); }
    finally { busy(login, false); }
  });
  register.addEventListener('submit', async event => {
    event.preventDefault();
    const input = Object.fromEntries(new FormData(register));
    if (!/^[A-Za-z0-9._@-]{1,64}$/.test(input.username.trim())) { message(text('invalidUsername')); register.elements.username.focus(); return; }
    if (input.password !== input.confirm) { message(text('passwordsDiffer')); return; }
    message(''); busy(register, true);
    try {
      await api('/api/register', { method:'POST', body:JSON.stringify({ username:input.username, password:input.password }) });
      register.reset(); show('login'); login.elements.username.value = input.username;
      message(text('accountCreated')); login.elements.password.focus();
    } catch (error) { message(describe(error)); }
    finally { busy(register, false); }
  });
  return { showLogin(allowed) { registrationEnabled = allowed; show('login'); } };
}
