import { BrandLogo } from "./BrandLogo";

type AppError = {
  scope: string;
  message: string;
} | null;

type LoginScreenProps = {
  appError: AppError;
  canSubmitLogin: boolean;
  isConnecting: boolean;
  loginEmail: string;
  loginPassword: string;
  onConnect: () => void;
  onLoginEmailChange: (value: string) => void;
  onLoginPasswordChange: (value: string) => void;
  onTogglePassword: () => void;
  showPassword: boolean;
};

export function LoginScreen({
  appError,
  canSubmitLogin,
  isConnecting,
  loginEmail,
  loginPassword,
  onConnect,
  onLoginEmailChange,
  onLoginPasswordChange,
  onTogglePassword,
  showPassword
}: LoginScreenProps) {
  return (
    <main className="login-layout">
      <section className="panel login-hero-panel">
        <div className="login-hero-branding">
          <BrandLogo size="wide" />
          <div className="login-hero-caption">ALON SYSTEMS</div>
        </div>
      </section>

      <section className="panel login-panel">
        <div className="login-panel-header">
          <div>
            <div className="panel-kicker">Sentinel</div>
            <h2>Login</h2>
          </div>
        </div>

        <form
          className="login-form"
          onSubmit={(event) => {
            event.preventDefault();
            onConnect();
          }}
        >
          <label>
            <span>Email</span>
            <input
              type="email"
              value={loginEmail}
              onChange={(event) => onLoginEmailChange(event.target.value)}
              placeholder="name@example.com"
              autoComplete="username"
              inputMode="email"
              spellCheck={false}
            />
          </label>

          <label>
            <span>Password</span>
            <div className="password-row">
              <input
                type={showPassword ? "text" : "password"}
                value={loginPassword}
                onChange={(event) => onLoginPasswordChange(event.target.value)}
                placeholder="Password"
                autoComplete="current-password"
              />
              <button
                className="ghost-button password-toggle"
                type="button"
                onClick={onTogglePassword}
                aria-label={showPassword ? "Hide password" : "Show password"}
                title={showPassword ? "Hide password" : "Show password"}
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path
                    d="M2.25 12s3.75-6.75 9.75-6.75S21.75 12 21.75 12 18 18.75 12 18.75 2.25 12 2.25 12Z"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.8"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                  <circle
                    cx="12"
                    cy="12"
                    r="3"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.8"
                  />
                  {showPassword && (
                    <path
                      d="M4 20 20 4"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.8"
                      strokeLinecap="round"
                    />
                  )}
                </svg>
              </button>
            </div>
          </label>

          {appError?.scope === "connection" && <div className="error-banner">{appError.message}</div>}

          {import.meta.env.VITE_SHOW_DEMO_ACCOUNT === "true" && (
            <p className="login-demo-hint">
              Demo access: <strong>demo@alon.systems</strong> / <strong>Demo123$</strong>
            </p>
          )}

          <div className="panel-actions login-actions">
            <button className="primary-button" type="submit" disabled={!canSubmitLogin || isConnecting}>
              {isConnecting ? "Signing In..." : "Sign In"}
            </button>
          </div>
        </form>
      </section>
    </main>
  );
}
