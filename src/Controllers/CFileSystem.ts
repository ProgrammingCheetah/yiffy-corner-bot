import IFileSystem from '../Interfaces/IFileSystem';
import fs from 'fs';
import path from 'path';
type FileSystemImplementedTypes = IFileSystem;
class CFileSystem implements IFileSystem {
    getVaultPath(): string {
        return path.join(process.env.NODE_CONFIG_DIR!, 'vault');
    }
    readFile(pathToFile: string): Promise<string> {
        const actualPath = path.isAbsolute(pathToFile)
            ? pathToFile
            : path.join(__dirname, pathToFile);
        return new Promise((resolve, reject) => {
            fs.readFile(actualPath, 'utf8', (err, data) => {
                if (err) reject(err);
                else resolve(data);
            });
        });
    }

    readFileSync(pathToFile: string): string {
        return fs.readFileSync(pathToFile, { encoding: 'utf8' });
    }

    readFileArray(pathsToFiles: string[]): Promise<string>[] {
        return pathsToFiles.map(pathToFile => this.readFile(pathToFile));
    }

    readFileArraySync(pathsToFiles: string[]): string[] {
        return pathsToFiles.map(pathToFile => this.readFileSync(pathToFile));
    }

    readVaultFile(pathToFile: string): Promise<string> {
        return this.readFile(path.join(this.getVaultPath(), pathToFile));
    }

    readVaultFileArray(pathsToFiles: string[]): Promise<string>[] {
        return pathsToFiles.map(pathToFile => this.readVaultFile(pathToFile));
    }

    readVaultFileArraySync(pathsToFiles: string[]): string[] {
        return pathsToFiles.map(pathToFile => this.readVaultFileSync(pathToFile));
    }

    readVaultFileSync(pathToFile: string): string {
        return this.readFileSync(path.join(this.getVaultPath(), pathToFile));
    }

    writeVaultFile(pathToFile: string, newContent: string): Promise<boolean> {
        return new Promise((resolve, reject) => {
            const vaultPath = this.getVaultPath();
            if (!vaultPath) reject(new Error('Vault path not found'));
            fs.writeFile(path.join(vaultPath, pathToFile), newContent, (err) => {
                if (err) reject(err);
                else resolve(true);
            });
        });
    }

    writeVaultFileSync(pathToFile: string, newContent: string): boolean {
        const vaultPath = this.getVaultPath();
        if (!vaultPath) throw new Error('Vault path not found');
        fs.writeFileSync(path.join(vaultPath, pathToFile), newContent);
        return true;
    }
}

function GetComponent(): FileSystemImplementedTypes {
    return new CFileSystem();
}


export default { GetComponent };
