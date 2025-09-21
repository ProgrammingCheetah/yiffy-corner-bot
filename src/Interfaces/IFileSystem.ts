abstract class IFileSystem {
    abstract readFile(pathToFile: string): Promise<string>;
    abstract readFileSync(pathToFile: string): string;
    abstract readFileArray(pathsToFiles: string[]): Promise<string>[];
    abstract readFileArraySync(pathsToFiles: string[]): string[];
    abstract readVaultFile(pathToFile: string): Promise<string>;
    abstract readVaultFileSync(pathToFile: string): string;
    abstract readVaultFileArray(pathsToFiles: string[]): Promise<string>[];
    abstract readVaultFileArraySync(pathsToFiles: string[]): string[];
    abstract writeVaultFile(pathToFile: string, newContent: string): Promise<boolean>;
    abstract writeVaultFileSync(pathToFile: string, newContent: string): boolean;
    abstract getVaultPath(): string;
}

export default IFileSystem;