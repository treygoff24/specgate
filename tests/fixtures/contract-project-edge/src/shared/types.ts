export interface SharedType {
    id: string;
    value: number;
}

export function sharedUtil(): SharedType {
    return { id: "1", value: 42 };
}
