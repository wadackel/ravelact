// Global vitest setup. Adds jest-dom's custom matchers (toBeInTheDocument,
// toBeDisabled, toHaveAttribute, ...) to every test file so component tests
// can use them without per-file imports.
import "@testing-library/jest-dom/vitest";
