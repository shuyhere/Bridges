"use client";

import { cn } from "@/lib/cn";
import { InputHTMLAttributes, forwardRef } from "react";

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ className, label, error, id, ...props }, ref) => {
    return (
      <div className="space-y-1.5">
        {label && (
          <label htmlFor={id} className="block text-[13px] font-medium text-muted">
            {label}
          </label>
        )}
        <input
          ref={ref}
          id={id}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          data-1p-ignore
          data-lpignore="true"
          className={cn(
            "w-full rounded-[6px] border bg-white px-3.5 py-2.5 text-[14px] text-foreground",
            "placeholder:text-muted/50",
            "focus:outline-none focus:ring-2 focus:ring-accent/20 focus:border-accent/30",
            "transition-all",
            error ? "border-danger/40" : "border-border",
            className,
          )}
          {...props}
        />
        {error && <p className="text-[12px] text-danger">{error}</p>}
      </div>
    );
  },
);
Input.displayName = "Input";
