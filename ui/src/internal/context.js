// What a component shares with the ones inside it.

/** `<Form>` → its `<Field>`s and submit `<Button>`s: the errors, whether it's busy. */
export const FORM = Symbol("elyra:form");

/** `<Field>` → its control: the id, the label's id, the description, whether it's invalid. */
export const FIELD = Symbol("elyra:field");

/** `<Dropdown>` → its `<MenuItem>`s: how to close the menu. */
export const MENU = Symbol("elyra:menu");
