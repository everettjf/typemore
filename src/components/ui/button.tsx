import type { ButtonHTMLAttributes } from "react";
import { cn } from "../../lib/utils";

type ButtonVariant = "default" | "outline" | "destructive";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
};

const variantClass: Record<ButtonVariant, string> = {
  default:
    "border border-slate-900 bg-slate-900 text-white hover:bg-slate-800 hover:border-slate-800",
  outline:
    "border border-slate-300 bg-white text-slate-700 hover:bg-slate-50",
  destructive:
    "border border-red-600 bg-red-600 text-white hover:bg-red-700 hover:border-red-700",
};

export function Button({ className, variant = "default", ...props }: ButtonProps) {
  return (
    <button
      className={cn(
        "inline-flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium transition disabled:cursor-not-allowed disabled:opacity-60",
        variantClass[variant],
        className
      )}
      {...props}
    />
  );
}
