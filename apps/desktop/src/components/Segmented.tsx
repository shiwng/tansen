import { type ReactNode } from 'react';

/** Apple's segmented control, from the tokens (report §8): one pill of two or
 * three choices, the selected one raised. */
export function Segmented<T extends string>({
  value,
  options,
  onChange,
  className,
}: {
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (value: T) => void;
  className?: string;
}) {
  return (
    <div className={`inline-flex rounded-pill bg-segmented p-[2px] ${className ?? ''}`} role="tablist">
      {options.map(({ value: option, label }) => (
        <button
          key={option}
          role="tab"
          aria-selected={option === value}
          onClick={() => onChange(option)}
          className={`h-[26px] rounded-pill px-3 type-callout-emphasized transition-colors ${
            option === value ? 'bg-segmented-selected text-primary' : 'text-secondary hover:text-primary'
          }`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
