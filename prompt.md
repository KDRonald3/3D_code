I trust you to handle the extraction of stuff from the cst correctly.

I will assume that you are able to do that, and design the interfile/interfolder algo such that it can build the ast for the project correctly.

# Scope
The algo described here is only confined to functions.

# **Data Structure**
    A folder can have only files and folders as its children.
    a file can have only functions as its children
    
    There are 2 types of functions: Infile functions, and refrenced functions.

    A function can have only functions, comments and files(containing the declaration of the function) as it's children.

    A tree is complete only when all the leaf nodes are files or coments.

I trust that you will be able to develop a structure that will deal with the conflict of origin file, and file in folder. I also trust that you will be able to deal with the conflict between function declarations and function calls.

# **Creating the tree**



There are 2 cases: import through cargo.toml, and a standard crate import.

## **Case 1: Crate imports**

I am going to assume modules are thesame as file. It is true that is not the case, but you can always create a link between each module and the file it contains.

### Scenario 1:  With reference to file(crate::module/file::function)
When the function is added to the tree as a function call, the file can be extracted right away, and added as a child of the function.

### Scenario 2: without reference to the file(crate::function)
It is implicit here that if we go to lib.rs, the origins of the function can be traced back relatively easily given the function will have to be called/imported in lib.rs, you can just make a map of each function that is imported or modified in any way, and map it to the origin file.

### Scenario 3: crate::module/file::*
If there are multiple of these, make a map of all the declared functions in each of these files, and map them to file of origin, then extract origin file from there. this is made much easier because you can't have name conflicts
I trust you can handle super::module/file::

## **Case 2: cargo.toml imports**

when a project is gotten, the first thing that is done is getting cargo.meta data. once that is done, you have to identify all dependencies and separate them into local and external ones. When working in files, if a function is found to be linked to external dependencies, you ignore it. if it is found to be linked to a local dependency, I believe everything in case one will be of use to find the correct file.

I have not said anything about comments because I believe that is easy for you to figure out.

ask questions to clarify things.