import { type ReactElement, type ReactNode } from "react";
import { AlertTriangle, Info, Loader2, XCircle } from "lucide-react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

import { Button as ShadcnButton } from "@/components/ui/button";
import {
  Card as ShadcnCard,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge as ShadcnBadge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";

export { Switch, Input };
export {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

// ---------------------------------------------------------------------------
// Button
// ---------------------------------------------------------------------------

export function Button({
  children,
  variant = "default",
  size = "md",
  className = "",
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "default" | "primary" | "danger" | "destructive" | "outline" | "ghost" | "link";
  size?: "sm" | "md" | "lg";
}) {
  const mappedVariant =
    variant === "primary"
      ? "default"
      : variant === "danger" || variant === "destructive"
        ? "destructive"
        : variant === "outline" || variant === "ghost" || variant === "link"
          ? variant
          : "outline";
  const mappedSize = size === "sm" ? "sm" : size === "lg" ? "lg" : "default";
  return (
    <ShadcnButton variant={mappedVariant} size={mappedSize} className={className} {...props}>
      {children}
    </ShadcnButton>
  );
}

// ---------------------------------------------------------------------------
// Helper tooltips (restore the reference app's explanatory hints)
// ---------------------------------------------------------------------------

export function Tip({
  title,
  body,
  children,
}: {
  title: string;
  body: string;
  children: ReactElement;
}) {
  return (
    <Tooltip>
      <TooltipTrigger render={children} />
      <TooltipContent className="max-w-xs">
        <p className="text-xs font-medium">{title}</p>
        <p className="mt-1 text-xs">{body}</p>
      </TooltipContent>
    </Tooltip>
  );
}

export function HelpTip({ title, body }: { title: string; body: string }) {
  return (
    <Tooltip>
      <TooltipTrigger
        type="button"
        aria-label={title}
        className="inline-flex size-5 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      >
        <Info size={12} />
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">
        <p className="text-xs font-medium">{title}</p>
        <p className="mt-1 text-xs">{body}</p>
      </TooltipContent>
    </Tooltip>
  );
}

export function TipButton({
  tipTitle,
  tipBody,
  children,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "default" | "primary" | "danger" | "destructive" | "outline" | "ghost" | "link";
  size?: "sm" | "md" | "lg";
  tipTitle: string;
  tipBody: string;
}) {
  return (
    <Tip title={tipTitle} body={tipBody}>
      <Button {...props}>{children}</Button>
    </Tip>
  );
}

// ---------------------------------------------------------------------------
// Card (ergonomic wrapper)
// ---------------------------------------------------------------------------

export function Card({
  title,
  subtitle,
  actions,
  children,
  className = "",
}: {
  title?: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <ShadcnCard className={className}>
      {(title || subtitle || actions) && (
        <CardHeader>
          <div className="min-w-0 flex-1">
            {subtitle ? <CardDescription>{subtitle}</CardDescription> : null}
            {title ? <CardTitle>{title}</CardTitle> : null}
          </div>
          {actions ? <CardAction>{actions}</CardAction> : null}
        </CardHeader>
      )}
      <CardContent>{children}</CardContent>
    </ShadcnCard>
  );
}

// ---------------------------------------------------------------------------
// Badge
// ---------------------------------------------------------------------------

export function Badge({
  tone = "neutral",
  children,
}: {
  tone?: "neutral" | "ok" | "warn" | "bad" | "info";
  children: ReactNode;
}) {
  const toneClass = {
    neutral: "bg-secondary text-secondary-foreground",
    ok: "border-transparent bg-[var(--ok-soft)] text-[var(--ok)]",
    warn: "border-transparent bg-[var(--warn-soft)] text-[var(--warn)]",
    bad: "border-transparent bg-[var(--danger-soft)] text-destructive",
    info: "border-transparent bg-[var(--info-soft)] text-[var(--info)]",
  }[tone];
  return (
    <ShadcnBadge variant="outline" className={toneClass}>
      {children}
    </ShadcnBadge>
  );
}

// ---------------------------------------------------------------------------
// Toggle row
// ---------------------------------------------------------------------------

export function ToggleRow({
  title,
  description,
  checked,
  onChange,
  disabled,
  badge,
  tipTitle,
  tipBody,
}: {
  title: string;
  description?: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  badge?: ReactNode;
  tipTitle?: string;
  tipBody?: string;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-2.5">
      <div className="min-w-0">
        <p className="flex items-center gap-2 text-[13px] font-medium">
          {title}
          {badge}
          {tipTitle && tipBody ? <HelpTip title={tipTitle} body={tipBody} /> : null}
        </p>
        {description && (
          <p className="mt-0.5 text-xs" style={{ color: "var(--muted-text)" }}>
            {description}
          </p>
        )}
      </div>
      <Switch
        checked={checked}
        onCheckedChange={(value) => onChange(value)}
        disabled={disabled}
        aria-label={title}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Feedback states
// ---------------------------------------------------------------------------

export function Spinner({ label = "Loading" }: { label?: string }) {
  return (
    <div
      role="status"
      aria-live="polite"
      className="flex items-center gap-2 text-sm"
      style={{ color: "var(--muted-text)" }}
    >
      <Loader2 size={16} className="animate-spin" />
      {label}
    </div>
  );
}

export function LoadingState({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="flex h-40 items-center justify-center">
      <Spinner label={label} />
    </div>
  );
}

export function EmptyState({ title, detail }: { title: string; detail?: string }) {
  return (
    <div
      className="flex h-40 flex-col items-center justify-center gap-1 rounded-lg"
      style={{ background: "var(--surface-2)" }}
    >
      <Info size={18} style={{ color: "var(--muted-text)" }} />
      <p className="text-sm font-medium text-foreground">{title}</p>
      {detail && <p className="text-xs text-muted-foreground">{detail}</p>}
    </div>
  );
}

export function ErrorState({
  title = "Something went wrong",
  detail,
  onRetry,
}: {
  title?: string;
  detail?: string;
  onRetry?: () => void;
}) {
  return (
    <div
      role="alert"
      className="flex h-40 flex-col items-center justify-center gap-2 rounded-lg"
      style={{ background: "var(--danger-soft)" }}
    >
      <XCircle size={20} style={{ color: "var(--danger)" }} />
      <p className="text-sm font-semibold text-foreground">{title}</p>
      {detail && <p className="max-w-md text-center text-xs text-muted-foreground">{detail}</p>}
      {onRetry && (
        <Button size="sm" onClick={onRetry}>
          Retry
        </Button>
      )}
    </div>
  );
}

export function WarningBanner({ children }: { children: ReactNode }) {
  return (
    <div
      className="flex items-start gap-2 rounded-lg px-3 py-2 text-sm"
      style={{ background: "var(--accent-soft)", color: "var(--text)" }}
    >
      <AlertTriangle size={16} className="mt-0.5 shrink-0" style={{ color: "var(--warn)" }} />
      <div className="min-w-0">{children}</div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Dialog
// ---------------------------------------------------------------------------

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  footer,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  children: ReactNode;
  footer?: ReactNode;
}) {
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {description ? <DialogDescription>{description}</DialogDescription> : null}
        </DialogHeader>
        <div className="text-sm" style={{ color: "var(--muted-text)" }}>
          {children}
        </div>
        {footer ? <DialogFooter>{footer}</DialogFooter> : null}
      </DialogContent>
    </Dialog>
  );
}

export {
  Dialog,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
};

export function KeyValue({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 py-1.5">
      <span className="min-w-0 break-words text-xs" style={{ color: "var(--muted-text)" }}>
        {label}
      </span>
      <span className="mono min-w-0 break-all text-right text-xs">{value}</span>
    </div>
  );
}
