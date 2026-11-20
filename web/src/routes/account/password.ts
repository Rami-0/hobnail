export function passwordErrors(current: string, password: string, confirm: string): Record<string, string> {
  const errors: Record<string, string> = {};
  if (!current) errors.current = "Enter your current password.";
  if ([...password].length < 8) errors.new = "Use at least 8 characters.";
  else if (new TextEncoder().encode(password).length > 1024) errors.new = "Use at most 1024 bytes.";
  else if (/\p{Cc}/u.test(password)) errors.new = "The password must not contain control characters.";
  if (password !== confirm) errors.confirm = "The passwords do not match.";
  return errors;
}
