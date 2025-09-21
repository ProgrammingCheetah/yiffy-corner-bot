import path from 'path';
import webpack from 'webpack';
import nodeExternals from 'webpack-node-externals';
import TerserPlugin from 'terser-webpack-plugin';

const config = {
    entry: {
        server: './src/index.ts',
    },
    watch: false,
    target: 'node',
    externals: [nodeExternals()],
    node: {
        __dirname: false,
        __filename: false,
    },
    module: {
        rules: [{ test: /\.ts$/, use: ['ts-loader'], exclude: [/node_modules/] }],
    },
    mode: 'development',
    resolve: {
        extensions: ['.ts', 'tsx'],
    },
    plugins: [new webpack.HotModuleReplacementPlugin()],
    optimization: {
        minimize: true,
        minimizer: [
            new TerserPlugin({
                parallel: 4,
                extractComments: true,
                terserOptions: {
                    sourceMap: true,
                },
            }),
        ],
    },
    output: {
        path: path.join(__dirname, 'dist'),
        publicPath: '/',
        filename: 'index.js',
    },
};
module.exports = config;
