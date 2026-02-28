import type { TextareaHTMLAttributes } from "react";
import { cn } from "../../lib/utils";

export function Textarea({ className, ...props }: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      className={cn(
        "min-h-0 flex-1 resize-none rounded-lg border border-slate-200 bg-slate-50/70 p-4 text-[15px] leading-7 outline-none ring-sky-300 transition focus:ring",
        className
      )}
      {...props}
    />
  );
}
