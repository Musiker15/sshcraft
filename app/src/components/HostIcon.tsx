import { Monitor, Server } from "lucide-react";
import {
  siAlmalinux,
  siAlpinelinux,
  siApple,
  siArchlinux,
  siCentos,
  siDebian,
  siFedora,
  siFreebsd,
  siLinux,
  siOpenbsd,
  siOpensuse,
  siRaspberrypi,
  siRedhat,
  siRockylinux,
  siUbuntu,
  type SimpleIcon,
} from "simple-icons";
import type { HostOs } from "../api";

const LOGOS: Partial<Record<HostOs, SimpleIcon>> = {
  linux: siLinux,
  debian: siDebian,
  ubuntu: siUbuntu,
  fedora: siFedora,
  red_hat: siRedhat,
  cent_os: siCentos,
  rocky: siRockylinux,
  alma: siAlmalinux,
  arch: siArchlinux,
  alpine: siAlpinelinux,
  open_suse: siOpensuse,
  raspbian: siRaspberrypi,
  free_bsd: siFreebsd,
  open_bsd: siOpenbsd,
  mac_os: siApple,
};

/** The logo of a host's operating system, or a neutral server symbol. */
export function HostIcon({ os, size = 18 }: { os?: HostOs | null; size?: number }) {
  const logo = os ? LOGOS[os] : undefined;
  if (logo) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <path d={logo.path} />
      </svg>
    );
  }
  if (os === "windows") return <Monitor size={size} aria-hidden="true" />;
  return <Server size={size} aria-hidden="true" />;
}
