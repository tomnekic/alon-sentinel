import logoSrc from "../assets/sentinel-logo.png";

type BrandLogoProps = {
  size?: "sm" | "lg" | "wide";
};

export function BrandLogo({ size = "sm" }: BrandLogoProps) {
  return (
    <div className={`brand-logo brand-logo-${size}`}>
      <img src={logoSrc} alt="Alon Sentinel" />
    </div>
  );
}
